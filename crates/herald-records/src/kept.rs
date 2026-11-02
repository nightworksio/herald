use herald_core::{
    Cursor, DestinationName, Entry, Key, RouteId, Schedule, Scheme, SourceName, Subscription,
};
use herald_ports::{
    Admitted, Capabilities, Keeping, Store, StoreError, Version, Versioned, Written,
};
use jiff::Timestamp;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::paths;
use crate::records::{Blob, Records, Wrote};

/// How often a write that lost a race to an unrelated write is tried again.
const ATTEMPTS: usize = 3;

/// A record as it is kept: its version beside it.
#[derive(Serialize, Deserialize)]
struct Stored {
    version: u64,
    record: Value,
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
}

fn corrupt(path: &str, error: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        reason: format!("{path}: {error}"),
    }
}

fn changing(path: &str, doing: &str) -> StoreError {
    StoreError::Unavailable {
        reason: format!("{path} kept changing while herald {doing} it"),
    }
}

impl<R: Records> Kept<R> {
    /// The store kept in `records`.
    #[must_use]
    pub fn new(records: R) -> Self {
        Self { records }
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
        let bytes = serde_json::to_vec(&stored).unwrap_or_default();
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
        let record = serde_json::to_value(value).unwrap_or(Value::Null);
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
        let record = serde_json::to_value(value).unwrap_or(Value::Null);
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
        let record = serde_json::to_value(value).unwrap_or(Value::Null);
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
            if let Some(record) = self.record::<T>(&path).await? {
                found.push(record.value);
            }
        }
        Ok(found)
    }
}

impl<R: Records> Store for Kept<R> {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            ledger: Keeping::Kept,
            cursors: true,
            schedules: Keeping::Kept,
            subscriptions: true,
            pins: true,
            deliveries: true,
            concurrent: true,
        }
    }

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

    async fn schedules(&self) -> Result<Vec<Schedule>, StoreError> {
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

    async fn subscribers(
        &self,
        destination: &DestinationName,
        project: &str,
    ) -> Result<Vec<String>, StoreError> {
        let subscriptions: Vec<Subscription> =
            self.all(&paths::subscribers(destination, project)).await?;
        Ok(subscriptions
            .into_iter()
            .map(|subscription| subscription.subscriber)
            .collect())
    }

    async fn subscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        self.added(&paths::subscribed(subscription), subscription)
            .await
    }

    async fn unsubscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        self.removed(&paths::subscribed(subscription)).await
    }

    async fn pinned(&self, route: &RouteId) -> Result<Option<String>, StoreError> {
        Ok(self
            .record::<String>(&paths::pin(route))
            .await?
            .map(|record| record.value))
    }

    async fn put_pinned(&self, route: &RouteId, post: &str) -> Result<(), StoreError> {
        self.kept(&paths::pin(route), &post).await
    }

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
            let admitted = self.record::<Timestamp>(&path).await?;
            if admitted.is_some_and(|admitted| admitted.value < before) {
                self.removed(&path).await?;
            }
        }
        Ok(())
    }
}
