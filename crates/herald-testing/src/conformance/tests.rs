use herald_core::State;
use herald_ports::{
    Admitted, Announcements, Cursors, Deliveries, Inboxes, Keeping, Keyring, KnownKey, Ledger,
    Notes, Pins, Schedules, Subscriptions, Versioned,
};
use jiff::Timestamp;

use super::*;
use crate::FakeStore;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    OverwritesEntries,
    RefusesWrites,
    OverwritesCursors,
    DuplicatesSchedules,
    IgnoresProjects,
    KeepsTheFirstPin,
    SharesPins,
    HoldsAnUnwrittenDelivery,
    KeepsTheFirstDeliveryState,
    SharesInboxes,
    AdmitsTwice,
    SharesDeliveries,
    KeepsReleased,
    ExpiresEverything,
    ExpiresNothing,
    KeepsTheFirstNote,
    SharesNotes,
    OverwritesLatest,
    ListsNoLatest,
    ListsAnUnwrittenLatest,
    Broken,
    KeepsNothing,
    DerivesButKeeps,
    DerivesSchedulesButKeeps,
    DerivesAtOneVersion,
    Derives,
    SkipsWhatItNeverHeld,
}

struct Faulty {
    fault: Fault,
    inner: FakeStore,
}

fn broken() -> StoreError {
    StoreError::Unavailable {
        reason: "broken".to_owned(),
    }
}

impl Faulty {
    fn new(fault: Fault) -> Self {
        Self {
            fault,
            inner: FakeStore::default(),
        }
    }

    fn refuses(&self) -> Result<(), StoreError> {
        match self.fault {
            Fault::Broken => Err(broken()),
            _ => Ok(()),
        }
    }

    fn keeps(&self) -> Option<&Self> {
        (self.fault != Fault::KeepsNothing).then_some(self)
    }
}

impl Faulty {
    fn inbox<'a>(&self, inbox: &'a str) -> &'a str {
        match self.fault {
            Fault::SharesInboxes => "shared",
            _ => inbox,
        }
    }
}

impl Inboxes for Faulty {
    async fn delivery(&self, key: &Key, inbox: &str) -> Result<Option<State>, StoreError> {
        self.refuses()?;
        let kept = self.inner.delivery(key, self.inbox(inbox)).await?;
        if self.fault == Fault::HoldsAnUnwrittenDelivery && kept.is_none() {
            return Ok(Some(State::Attempted { at: NOON }));
        }
        Ok(kept)
    }

    async fn put_delivery(&self, key: &Key, inbox: &str, state: &State) -> Result<(), StoreError> {
        self.refuses()?;
        let inbox = self.inbox(inbox);
        let kept = self.inner.delivery(key, inbox).await?;
        if self.fault == Fault::KeepsTheFirstDeliveryState && kept.is_some() {
            return Ok(());
        }
        self.inner.put_delivery(key, inbox, state).await
    }
}

impl Ledger for Faulty {
    fn keeping(&self) -> Keeping {
        match self.fault {
            Fault::DerivesButKeeps | Fault::DerivesAtOneVersion | Fault::Derives => {
                Keeping::Derived
            }
            _ => Keeping::Kept,
        }
    }

    async fn entry(&self, key: &Key) -> Result<Option<Versioned<Entry>>, StoreError> {
        self.refuses()?;
        if self.fault == Fault::Derives {
            return Ok(None);
        }
        self.inner.entry(key).await
    }

    async fn put_entry(
        &self,
        entry: &Entry,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.refuses()?;
        match self.fault {
            Fault::RefusesWrites => return Ok(Written::Conflict),
            Fault::DerivesAtOneVersion => return Ok(Written::Written(Version(1))),
            Fault::Derives => {
                return Ok(Written::Written(Version(expected.map_or(1, |v| v.0 + 1))));
            }
            _ => {}
        }
        let expected = match self.fault {
            Fault::OverwritesEntries => self
                .inner
                .entry(&entry.key)
                .await?
                .map(|record| record.version),
            _ => expected,
        };
        self.inner.put_entry(entry, expected).await
    }
}

impl Cursors for Faulty {
    async fn cursor(&self, source: &SourceName) -> Result<Option<Versioned<Cursor>>, StoreError> {
        self.refuses()?;
        self.inner.cursor(source).await
    }

    async fn put_cursor(
        &self,
        source: &SourceName,
        cursor: &Cursor,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.refuses()?;
        let expected = match self.fault {
            Fault::OverwritesCursors => self
                .inner
                .cursor(source)
                .await?
                .map(|record| record.version),
            _ => expected,
        };
        self.inner.put_cursor(source, cursor, expected).await
    }
}

impl Schedules for Faulty {
    fn keeping(&self) -> Keeping {
        match self.fault {
            Fault::DerivesSchedulesButKeeps | Fault::Derives => Keeping::Derived,
            _ => Keeping::Kept,
        }
    }

    async fn every_schedule(&self) -> Result<Vec<Schedule>, StoreError> {
        self.refuses()?;
        if self.fault == Fault::Derives {
            return Ok(Vec::new());
        }
        let held = self.inner.every_schedule().await?;
        Ok(match self.fault {
            Fault::DuplicatesSchedules => held.iter().chain(&held).cloned().collect(),
            _ => held,
        })
    }

    async fn put_schedules(&self, add: &[Schedule], remove: &[Schedule]) -> Result<(), StoreError> {
        self.refuses()?;
        self.inner.put_schedules(add, remove).await
    }
}

impl Subscriptions for Faulty {
    async fn subscribers(
        &self,
        destination: &DestinationName,
        project: &str,
    ) -> Result<Vec<Subscription>, StoreError> {
        self.refuses()?;
        let mut held = self.inner.subscribers(destination, project).await?;
        if self.fault == Fault::IgnoresProjects {
            held.extend(self.inner.subscribers(destination, "other").await?);
        }
        Ok(held)
    }

    async fn subscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        self.refuses()?;
        self.inner.subscribe(subscription).await
    }

    async fn unsubscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        self.refuses()?;
        self.inner.unsubscribe(subscription).await
    }
}

impl Pins for Faulty {
    async fn pinned(&self, route: &RouteId) -> Result<Option<String>, StoreError> {
        self.refuses()?;
        let route = match self.fault {
            Fault::SharesPins => RouteId::new("shared"),
            _ => route.clone(),
        };
        self.inner.pinned(&route).await
    }

    async fn put_pinned(&self, route: &RouteId, post: &str) -> Result<(), StoreError> {
        self.refuses()?;
        let route = match self.fault {
            Fault::SharesPins => RouteId::new("shared"),
            _ => route.clone(),
        };
        let kept = self.inner.pinned(&route).await?;
        if self.fault == Fault::KeepsTheFirstPin && kept.is_some() {
            return Ok(());
        }
        self.inner.put_pinned(&route, post).await
    }
}

impl Deliveries for Faulty {
    async fn admit(&self, scheme: Scheme, id: &str, at: Timestamp) -> Result<Admitted, StoreError> {
        self.refuses()?;
        let scheme = match self.fault {
            Fault::SharesDeliveries => Scheme::GitHub,
            _ => scheme,
        };
        let admitted = self.inner.admit(scheme, id, at).await?;
        Ok(match self.fault {
            Fault::AdmitsTwice => Admitted::Fresh,
            _ => admitted,
        })
    }

    async fn release(&self, scheme: Scheme, id: &str) -> Result<(), StoreError> {
        self.refuses()?;
        if self.fault == Fault::KeepsReleased {
            return Ok(());
        }
        self.inner.release(scheme, id).await
    }

    async fn expire_deliveries(&self, before: Timestamp) -> Result<(), StoreError> {
        self.refuses()?;
        match self.fault {
            Fault::ExpiresNothing => Ok(()),
            Fault::ExpiresEverything => self.inner.expire_deliveries(Timestamp::MAX).await,
            _ => self.inner.expire_deliveries(before).await,
        }
    }
}

impl Notes for Faulty {
    async fn note(&self, key: &Key) -> Result<Option<String>, StoreError> {
        self.refuses()?;
        let key = match self.fault {
            Fault::SharesNotes => super::key("shared"),
            _ => *key,
        };
        self.inner.note(&key).await
    }

    async fn put_note(&self, key: &Key, note: &str) -> Result<(), StoreError> {
        self.refuses()?;
        let key = match self.fault {
            Fault::SharesNotes => super::key("shared"),
            _ => *key,
        };
        let kept = self.inner.note(&key).await?;
        if self.fault == Fault::KeepsTheFirstNote && kept.is_some() {
            return Ok(());
        }
        self.inner.put_note(&key, note).await
    }
}

impl Announcements for Faulty {
    async fn latest(&self, project: &str) -> Result<Option<Versioned<Latest>>, StoreError> {
        self.refuses()?;
        self.inner.latest(project).await
    }

    async fn every_latest(&self) -> Result<Vec<Latest>, StoreError> {
        self.refuses()?;
        if self.fault == Fault::ListsNoLatest {
            return Ok(Vec::new());
        }
        if self.fault == Fault::ListsAnUnwrittenLatest {
            return Ok(vec![announced("unwritten", "0.1.0")]);
        }
        self.inner.every_latest().await
    }

    async fn put_latest(
        &self,
        latest: &Latest,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.refuses()?;
        let expected = match self.fault {
            Fault::OverwritesLatest => self
                .inner
                .latest(&latest.project)
                .await?
                .map(|kept| kept.version),
            _ => expected,
        };
        self.inner.put_latest(latest, expected).await
    }
}

impl Store for Faulty {
    type Ledger = Self;
    type Cursors = Self;
    type Schedules = Self;
    type Subscriptions = Self;
    type Pins = Self;
    type Deliveries = Self;
    type Notes = Self;
    type Announcements = Self;
    type Inboxes = Self;

    fn ledger(&self) -> Option<&Self> {
        self.keeps()
    }

    fn cursors(&self) -> Option<&Self> {
        self.keeps()
    }

    fn schedules(&self) -> Option<&Self> {
        self.keeps()
    }

    fn subscriptions(&self) -> Option<&Self> {
        self.keeps()
    }

    fn pins(&self) -> Option<&Self> {
        self.keeps()
    }

    fn deliveries(&self) -> Option<&Self> {
        self.keeps()
    }

    fn notes(&self) -> Option<&Self> {
        self.keeps()
    }

    fn announcements(&self) -> Option<&Self> {
        self.keeps()
    }

    fn inboxes(&self) -> Option<&Self> {
        self.keeps()
    }

    fn skipped(&self) -> Vec<String> {
        if self.fault == Fault::SkipsWhatItNeverHeld {
            vec!["corrupt.json".to_owned()]
        } else {
            self.inner.skipped()
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Forgetting {
    Correctly,
    Nothing,
    Everything,
    TheFirstKnown,
    OneKeyForAll,
    Broken,
}

struct FaultyKeyring {
    fault: Forgetting,
    keys: std::sync::Mutex<std::collections::BTreeMap<String, KnownKey>>,
}

impl FaultyKeyring {
    fn new(fault: Forgetting) -> Self {
        Self {
            fault,
            keys: std::sync::Mutex::default(),
        }
    }

    fn id(&self, id: &str) -> String {
        match self.fault {
            Forgetting::OneKeyForAll => "one".to_owned(),
            _ => id.to_owned(),
        }
    }

    fn broken(&self) -> Result<(), StoreError> {
        match self.fault {
            Forgetting::Broken => Err(broken()),
            _ => Ok(()),
        }
    }
}

impl Keyring for FaultyKeyring {
    async fn known(&self, id: &str) -> Result<Option<KnownKey>, StoreError> {
        self.broken()?;
        Ok(crate::lock(&self.keys).get(&self.id(id)).cloned())
    }

    async fn know(&self, id: &str, key: &KnownKey) -> Result<(), StoreError> {
        let mut keys = crate::lock(&self.keys);
        let id = self.id(id);
        if self.fault == Forgetting::TheFirstKnown && keys.contains_key(&id) {
            return Ok(());
        }
        keys.insert(id, key.clone());
        Ok(())
    }

    async fn forget_before(&self, before: Timestamp) -> Result<(), StoreError> {
        let before = match self.fault {
            Forgetting::Nothing => Timestamp::MIN,
            Forgetting::Everything => Timestamp::MAX,
            _ => before,
        };
        crate::lock(&self.keys).retain(|_, key| key.fetched >= before);
        Ok(())
    }
}

fn keyring(fault: Forgetting) -> Result<(), Failure> {
    pollster::block_on(keyring_conformance(&FaultyKeyring::new(fault)))
}

#[test]
fn a_keyring_that_keeps_its_promises_passes() {
    assert_eq!(keyring(Forgetting::Correctly), Ok(()));
}

#[test]
fn a_keyring_that_breaks_its_promises_is_refused() {
    let cases = [
        (
            Forgetting::Nothing,
            "a key read before the expiry was still known",
        ),
        (
            Forgetting::Everything,
            "a key read at the expiry was forgotten",
        ),
        (
            Forgetting::TheFirstKnown,
            "a key's id did not name the key known last",
        ),
        (
            Forgetting::OneKeyForAll,
            "a key's id did not name the key known last",
        ),
        (
            Forgetting::Broken,
            "the store failed: the store could not be reached: broken",
        ),
    ];
    for (fault, failure) in cases {
        assert_eq!(keyring(fault), Err(Failure(failure.to_owned())));
    }
}

fn run(fault: Fault) -> Result<(), Failure> {
    pollster::block_on(store_conformance(&Faulty::new(fault)))
}

#[test]
fn the_fake_store_passes() {
    assert_eq!(
        pollster::block_on(store_conformance(&FakeStore::default())),
        Ok(())
    );
}

#[test]
fn a_store_deriving_its_ledger_and_schedules_passes_when_it_keeps_neither() {
    assert_eq!(run(Fault::Derives), Ok(()));
    let cases = [
        (Fault::DerivesButKeeps, "a derived ledger kept a write"),
        (
            Fault::DerivesAtOneVersion,
            "a write to a derived ledger was not taken at a later version",
        ),
        (
            Fault::DerivesSchedulesButKeeps,
            "a store deriving schedules kept one written",
        ),
    ];
    for (fault, failure) in cases {
        assert_eq!(run(fault), Err(Failure(failure.to_owned())));
    }
}

#[test]
fn a_store_keeping_nothing_passes_when_it_says_so() {
    assert_eq!(run(Fault::KeepsNothing), Ok(()));
}

#[test]
fn a_store_that_breaks_its_promises_is_refused() {
    let cases = [
        (
            Fault::SkipsWhatItNeverHeld,
            "a store holding no record skipped one",
        ),
        (
            Fault::OverwritesEntries,
            "an entry was written as new over one that exists",
        ),
        (
            Fault::RefusesWrites,
            "a write the store should have taken conflicted",
        ),
        (
            Fault::OverwritesCursors,
            "a cursor was written over a version it did not expect",
        ),
        (
            Fault::DuplicatesSchedules,
            "schedules added were not held once each",
        ),
        (
            Fault::IgnoresProjects,
            "the subscribers of a project on a destination were not those subscribed, once each",
        ),
        (
            Fault::KeepsTheFirstPin,
            "a route's pin was not the one written last",
        ),
        (
            Fault::SharesPins,
            "a route's pin was not the one written last",
        ),
        (
            Fault::HoldsAnUnwrittenDelivery,
            "a new store holds a delivery to an inbox",
        ),
        (
            Fault::KeepsTheFirstDeliveryState,
            "a delivery to an inbox was not the state written last",
        ),
        (
            Fault::SharesInboxes,
            "a delivery to an inbox was not the state written last",
        ),
        (
            Fault::AdmitsTwice,
            "a delivery admitted before was fresh again",
        ),
        (
            Fault::SharesDeliveries,
            "another scheme's delivery of the same id was seen",
        ),
        (
            Fault::KeepsReleased,
            "a released delivery was not admitted again",
        ),
        (
            Fault::ExpiresNothing,
            "a delivery admitted before the expiry was still seen",
        ),
        (
            Fault::ExpiresEverything,
            "a delivery admitted at the expiry was forgotten",
        ),
        (
            Fault::KeepsTheFirstNote,
            "an announcement's note was not the one written last",
        ),
        (
            Fault::SharesNotes,
            "an announcement's note was not the one written last",
        ),
        (
            Fault::OverwritesLatest,
            "what a project last announced was written over a version it did not expect",
        ),
        (
            Fault::ListsNoLatest,
            "every project's latest was not listed as written last",
        ),
        (
            Fault::ListsAnUnwrittenLatest,
            "a new store holds what a project last announced",
        ),
    ];
    for (fault, failure) in cases {
        assert_eq!(run(fault), Err(Failure(failure.to_owned())));
    }
}

#[test]
fn a_failure_reads_as_its_sentence() {
    assert_eq!(Failure("x".to_owned()).to_string(), "x");
}

#[test]
fn a_store_that_fails_says_how() {
    assert_eq!(
        run(Fault::Broken),
        Err(Failure(
            "the store failed: the store could not be reached: broken".to_owned()
        ))
    );
}
