use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};

use herald_core::{
    Cursor, DestinationName, Entry, Key, Latest, RecordKind, RouteId, Schedule, Scheme, SourceName,
    State, Subscription,
};
use herald_ports::{
    Admitted, Announcements, Cursors, Deliveries, Held, Inboxes, Keeper, Keyring, KnownKey, Ledger,
    Notes, Pins, Schedules, StoreError, Subscriptions, Version, Versioned, Written,
};
use jiff::Timestamp;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::paths;
use crate::records::{Blob, Records, Wrote};
use crate::seal::Seal;

/// How often a write that lost a race to an unrelated write is tried again.
const ATTEMPTS: usize = 3;

/// A record as it is kept: its version beside it.
#[derive(Serialize, Deserialize)]
struct Stored {
    version: u64,
    record: Value,
}

/// A key as it is kept.
#[derive(Serialize, Deserialize)]
struct Known {
    owner: String,
    pem: String,
    fetched: Timestamp,
}

/// A Note as it is kept: when it was published beside it.
#[derive(Serialize, Deserialize)]
struct Published {
    note: String,
    at: Timestamp,
}

/// A record read, with what replacing it must name.
struct Read {
    token: String,
    version: Version,
    record: Value,
}

/// The `Store` kept in `R`, one record per path.
#[derive(Debug, Clone)]
pub struct Kept<R> {
    records: R,
    skipped: Arc<Mutex<BTreeSet<String>>>,
    keeps: BTreeSet<RecordKind>,
    seal: Option<Seal>,
}

fn opened(seal: Option<&Seal>, path: &str, bytes: Vec<u8>) -> Result<Vec<u8>, StoreError> {
    match seal {
        Some(seal) => seal.opened(path, &bytes),
        None => Ok(bytes),
    }
}

fn sealed(seal: Option<&Seal>, path: &str, bytes: Vec<u8>) -> Vec<u8> {
    match seal {
        Some(seal) => seal.sealed(path, &bytes),
        None => bytes,
    }
}

fn corrupt(path: &str, error: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        reason: format!("{path}: {error}"),
    }
}

fn encoded(path: &str, value: &impl Serialize) -> Result<Value, StoreError> {
    serde_json::to_value(value).map_err(|error| corrupt(path, error))
}

fn changing(path: &str, doing: &str) -> StoreError {
    StoreError::Unavailable {
        reason: format!("{path} kept changing while herald {doing} it"),
    }
}

impl<R: Records> Kept<R> {
    /// The store kept in `records`, keeping every kind of record.
    #[must_use]
    pub fn new(records: R) -> Self {
        Self {
            records,
            skipped: Arc::default(),
            keeps: [
                RecordKind::Subscriptions,
                RecordKind::Pins,
                RecordKind::Notes,
                RecordKind::Latest,
            ]
            .into(),
            seal: None,
        }
    }

    /// The store, sealing every record it writes with `seal` where there is
    /// one, and reading as corrupt every record that is not sealed with it
    /// (ADR-0005).
    #[must_use]
    pub fn sealed(mut self, seal: impl Into<Option<Seal>>) -> Self {
        self.seal = seal.into();
        self
    }

    /// The store, keeping besides its ledger, cursors, schedules and
    /// deliveries only the kinds `keeps` lets it. Deliveries to followers'
    /// inboxes are kept with subscriptions.
    #[must_use]
    pub fn keeping(mut self, keeps: impl Fn(RecordKind) -> bool) -> Self {
        self.keeps.retain(|kind| keeps(*kind));
        self
    }

    /// The record at `path` as a listing reads it: one that cannot be read
    /// is skipped, and its path kept to report.
    async fn listed<T: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<Option<Versioned<T>>, StoreError> {
        match self.record::<T>(path).await {
            Err(StoreError::Corrupt { .. }) => {
                self.skipped
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(path.to_owned());
                Ok(None)
            }
            read => read,
        }
    }

    /// The backend it keeps its records in.
    #[must_use]
    pub fn records(&self) -> &R {
        &self.records
    }

    async fn read(&self, path: &str) -> Result<Option<Read>, StoreError> {
        let Some(Blob { token, bytes }) = self.records.read(path).await? else {
            return Ok(None);
        };
        let bytes = opened(self.seal.as_ref(), path, bytes)?;
        let stored: Stored =
            serde_json::from_slice(&bytes).map_err(|error| corrupt(path, error))?;
        Ok(Some(Read {
            token,
            version: Version(stored.version),
            record: stored.record,
        }))
    }

    async fn write(
        &self,
        path: &str,
        version: Version,
        record: &Value,
        replacing: Option<&str>,
    ) -> Result<Wrote, StoreError> {
        let stored = Stored {
            version: version.0,
            record: record.clone(),
        };
        let bytes = sealed(
            self.seal.as_ref(),
            path,
            encoded(path, &stored)?.to_string().into_bytes(),
        );
        self.records.write(path, &bytes, replacing).await
    }

    async fn record<T: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<Option<Versioned<T>>, StoreError> {
        let Some(read) = self.read(path).await? else {
            return Ok(None);
        };
        let value = serde_json::from_value(read.record).map_err(|error| corrupt(path, error))?;
        Ok(Some(Versioned {
            value,
            version: read.version,
        }))
    }

    async fn compare_and_set<T: Serialize>(
        &self,
        path: &str,
        value: &T,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let record = encoded(path, value)?;
        for _ in 0..ATTEMPTS {
            let read = self.read(path).await?;
            if read.as_ref().map(|read| read.version) != expected {
                return Ok(Written::Conflict);
            }
            let version = Version(expected.map_or(1, |Version(number)| number + 1));
            let token = read.as_ref().map(|read| read.token.as_str());
            if self.write(path, version, &record, token).await? == Wrote::Written {
                return Ok(Written::Written(version));
            }
        }
        Ok(Written::Conflict)
    }

    async fn kept<T: Serialize>(&self, path: &str, value: &T) -> Result<(), StoreError> {
        let record = encoded(path, value)?;
        for _ in 0..ATTEMPTS {
            let read = self.read(path).await?;
            let version = read.as_ref().map_or(Version(1), |read| read.version);
            let token = read.as_ref().map(|read| read.token.as_str());
            if self.write(path, version, &record, token).await? == Wrote::Written {
                return Ok(());
            }
        }
        Err(changing(path, "wrote"))
    }

    async fn added<T: Serialize>(&self, path: &str, value: &T) -> Result<(), StoreError> {
        if self.records.read(path).await?.is_some() {
            return Ok(());
        }
        let record = encoded(path, value)?;
        self.write(path, Version(1), &record, None)
            .await
            .map(|_| ())
    }

    async fn removed(&self, path: &str) -> Result<(), StoreError> {
        for _ in 0..ATTEMPTS {
            let Some(Blob { token, .. }) = self.records.read(path).await? else {
                return Ok(());
            };
            if self.records.remove(path, &token).await? == Wrote::Written {
                return Ok(());
            }
        }
        Err(changing(path, "removed"))
    }

    async fn all<T: DeserializeOwned>(&self, directory: &str) -> Result<Vec<T>, StoreError> {
        let mut found = Vec::new();
        for path in self.records.list(directory).await? {
            if let Some(record) = self.listed::<T>(&path).await? {
                found.push(record.value);
            }
        }
        Ok(found)
    }
}

impl<R: Records> Inboxes for Kept<R> {
    async fn delivery(&self, key: &Key, inbox: &str) -> Result<Option<State>, StoreError> {
        Ok(self
            .record::<State>(&paths::inbox(key, inbox))
            .await?
            .map(|record| record.value))
    }

    async fn put_delivery(&self, key: &Key, inbox: &str, state: &State) -> Result<(), StoreError> {
        self.kept(&paths::inbox(key, inbox), state).await
    }

    async fn forget_deliveries(&self, key: &Key) -> Result<(), StoreError> {
        for path in self.records.list(&paths::inboxes(key)).await? {
            self.removed(&path).await?;
        }
        Ok(())
    }
}

impl<R: Records> Ledger for Kept<R> {
    async fn entry(&self, key: &Key) -> Result<Option<Versioned<Entry>>, StoreError> {
        self.record(&paths::entry(key)).await
    }

    async fn put_entry(
        &self,
        entry: &Entry,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.compare_and_set(&paths::entry(&entry.key), entry, expected)
            .await
    }

    async fn expire_entries(&self, before: Timestamp) -> Result<Vec<Key>, StoreError> {
        let mut expired = Vec::new();
        for path in self.records.list(paths::LEDGER).await? {
            let Some(entry) = self.listed::<Entry>(&path).await? else {
                continue;
            };
            if entry.value.expired(before) {
                self.removed(&path).await?;
                expired.push(entry.value.key);
            }
        }
        Ok(expired)
    }
}

impl<R: Records> Cursors for Kept<R> {
    async fn cursor(&self, source: &SourceName) -> Result<Option<Versioned<Cursor>>, StoreError> {
        self.record(&paths::cursor(source)).await
    }

    async fn put_cursor(
        &self,
        source: &SourceName,
        cursor: &Cursor,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.compare_and_set(&paths::cursor(source), cursor, expected)
            .await
    }
}

impl<R: Records> Schedules for Kept<R> {
    async fn every_schedule(&self) -> Result<Vec<Schedule>, StoreError> {
        self.all(paths::SCHEDULES).await
    }

    async fn put_schedules(&self, add: &[Schedule], remove: &[Schedule]) -> Result<(), StoreError> {
        for schedule in remove {
            self.removed(&paths::schedule(schedule)).await?;
        }
        for schedule in add {
            self.added(&paths::schedule(schedule), schedule).await?;
        }
        Ok(())
    }
}

impl<R: Records> Subscriptions for Kept<R> {
    async fn subscribers(
        &self,
        destination: &DestinationName,
        project: &str,
    ) -> Result<Vec<Subscription>, StoreError> {
        self.all(&paths::subscribers(destination, project)).await
    }

    async fn subscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        self.added(&paths::subscribed(subscription), subscription)
            .await
    }

    async fn unsubscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        self.removed(&paths::subscribed(subscription)).await
    }
}

impl<R: Records> Pins for Kept<R> {
    async fn pinned(&self, route: &RouteId) -> Result<Option<String>, StoreError> {
        Ok(self
            .record::<String>(&paths::pin(route))
            .await?
            .map(|record| record.value))
    }

    async fn put_pinned(&self, route: &RouteId, post: &str) -> Result<(), StoreError> {
        self.kept(&paths::pin(route), &post).await
    }
}

impl<R: Records> Deliveries for Kept<R> {
    async fn admit(&self, scheme: Scheme, id: &str, at: Timestamp) -> Result<Admitted, StoreError> {
        Ok(
            match self
                .compare_and_set(&paths::delivery(scheme, id), &at, None)
                .await?
            {
                Written::Written(_) => Admitted::Fresh,
                Written::Conflict => Admitted::Seen,
            },
        )
    }

    async fn release(&self, scheme: Scheme, id: &str) -> Result<(), StoreError> {
        self.removed(&paths::delivery(scheme, id)).await
    }

    async fn expire_deliveries(&self, before: Timestamp) -> Result<(), StoreError> {
        for path in self.records.list(paths::DELIVERIES).await? {
            let admitted = self.listed::<Timestamp>(&path).await?;
            if admitted.is_some_and(|admitted| admitted.value < before) {
                self.removed(&path).await?;
            }
        }
        Ok(())
    }
}

impl<R: Records> Notes for Kept<R> {
    async fn note(&self, key: &Key) -> Result<Option<String>, StoreError> {
        Ok(self
            .record::<Published>(&paths::note(key))
            .await?
            .map(|record| record.value.note))
    }

    async fn put_note(&self, key: &Key, note: &str, at: Timestamp) -> Result<(), StoreError> {
        let published = Published {
            note: note.to_owned(),
            at,
        };
        self.kept(&paths::note(key), &published).await
    }

    async fn expire_notes(&self, before: Timestamp) -> Result<(), StoreError> {
        for path in self.records.list(paths::NOTES).await? {
            let published = self.listed::<Published>(&path).await?;
            if published.is_some_and(|published| published.value.at < before) {
                self.removed(&path).await?;
            }
        }
        Ok(())
    }
}

impl<R: Records> Announcements for Kept<R> {
    async fn latest(&self, project: &str) -> Result<Option<Versioned<Latest>>, StoreError> {
        self.record(&paths::latest(project)).await
    }

    async fn every_latest(&self) -> Result<Vec<Latest>, StoreError> {
        self.all(paths::LATEST).await
    }

    async fn put_latest(
        &self,
        latest: &Latest,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.compare_and_set(&paths::latest(&latest.project), latest, expected)
            .await
    }
}

impl<R: Records> Keeper for Kept<R> {
    fn holds(&self, kind: Held) -> bool {
        match kind {
            Held::Ledger | Held::Cursors | Held::Schedules | Held::Deliveries => true,
            Held::Subscriptions | Held::Inboxes => self.keeps.contains(&RecordKind::Subscriptions),
            Held::Pins => self.keeps.contains(&RecordKind::Pins),
            Held::Notes => self.keeps.contains(&RecordKind::Notes),
            Held::Latest => self.keeps.contains(&RecordKind::Latest),
        }
    }

    fn skipped(&self) -> Vec<String> {
        std::mem::take(&mut *self.skipped.lock().unwrap_or_else(PoisonError::into_inner))
            .into_iter()
            .collect()
    }
}

impl<R: Records> Keyring for Kept<R> {
    async fn known(&self, id: &str) -> Result<Option<KnownKey>, StoreError> {
        Ok(self
            .record::<Known>(&paths::key(id))
            .await?
            .map(|record| KnownKey {
                owner: record.value.owner,
                pem: record.value.pem,
                fetched: record.value.fetched,
            }))
    }

    async fn know(&self, id: &str, key: &KnownKey) -> Result<(), StoreError> {
        let known = Known {
            owner: key.owner.clone(),
            pem: key.pem.clone(),
            fetched: key.fetched,
        };
        self.kept(&paths::key(id), &known).await
    }

    async fn forget_before(&self, before: Timestamp) -> Result<(), StoreError> {
        for path in self.records.list(paths::KEYS).await? {
            let known = self.listed::<Known>(&path).await?;
            if known.is_some_and(|known| known.value.fetched < before) {
                self.removed(&path).await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde::Serializer;
    use serde::ser::Error;

    use super::*;
    use crate::Memory;

    struct Unwritable;

    impl Serialize for Unwritable {
        fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(S::Error::custom("no JSON for this"))
        }
    }

    fn corrupted(reason: &str) -> StoreError {
        StoreError::Corrupt {
            reason: reason.to_owned(),
        }
    }

    #[test]
    fn a_sealed_store_keeps_every_record_and_trusts_none_it_did_not_seal() {
        let seal = || Seal::new(herald_ports::Secret::new("HERALD_STORE_KEY", "k"));
        let sealed = Kept::new(Memory::default()).sealed(seal());
        pollster::block_on(async {
            herald_testing::store_conformance(&sealed).await.unwrap();
            herald_testing::keyring_conformance(&Kept::new(Memory::default()).sealed(seal()))
                .await
                .unwrap();
            let subscription = Subscription {
                destination: DestinationName::new("slack"),
                project: "p".to_owned(),
                subscriber: "u".to_owned(),
                inbox: None,
            };
            let path = paths::subscribed(&subscription);
            let planted = br#"{"version":1,"record":{"destination":"slack","project":"p","subscriber":"mallory"}}"#;
            assert_eq!(
                sealed.records.write(&path, planted, None).await,
                Ok(Wrote::Written)
            );
            assert_eq!(
                sealed
                    .subscribers(&DestinationName::new("slack"), "p")
                    .await,
                Ok(vec![])
            );
            assert_eq!(sealed.skipped(), std::slice::from_ref(&path));
            assert_eq!(
                sealed.record::<Subscription>(&path).await.err(),
                Some(StoreError::Corrupt {
                    reason: format!("{path} is not sealed by this store's key")
                })
            );
        });
    }

    #[test]
    fn a_value_with_no_json_is_refused_as_corrupt_and_nothing_is_written() {
        let kept = Kept::new(Memory::default());
        pollster::block_on(async {
            assert_eq!(
                kept.compare_and_set("a.json", &Unwritable, None)
                    .await
                    .err(),
                Some(corrupted("a.json: no JSON for this"))
            );
            assert_eq!(
                kept.kept("b.json", &Unwritable).await.err(),
                Some(corrupted("b.json: no JSON for this"))
            );
            assert_eq!(
                kept.added("c.json", &Unwritable).await.err(),
                Some(corrupted("c.json: no JSON for this"))
            );
            for path in ["a.json", "b.json", "c.json"] {
                assert!(kept.records.read(path).await.unwrap().is_none());
            }
        });
    }
}
