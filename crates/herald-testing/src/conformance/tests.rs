use herald_ports::{Admitted, Capabilities, Keeping, Keyring, KnownKey, Versioned};
use jiff::Timestamp;

use super::*;
use crate::FakeStore;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    UndeclaredLedger,
    UndeclaredCursors,
    UndeclaredSchedules,
    UndeclaredSubscriptions,
    UndeclaredPins,
    UndeclaredDeliveries,
    UndeclaredNotes,
    UndeclaredLatest,
    OverwritesEntries,
    RefusesWrites,
    OverwritesCursors,
    DuplicatesSchedules,
    IgnoresProjects,
    KeepsTheFirstPin,
    SharesPins,
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

    fn refuses(&self, what: &'static str) -> Result<(), StoreError> {
        match self.fault {
            Fault::Broken => Err(broken()),
            Fault::KeepsNothing => Err(StoreError::Unsupported { what }),
            _ => Ok(()),
        }
    }
}

impl Store for Faulty {
    fn skipped(&self) -> Vec<String> {
        if self.fault == Fault::SkipsWhatItNeverHeld {
            vec!["corrupt.json".to_owned()]
        } else {
            self.inner.skipped()
        }
    }

    fn capabilities(&self) -> Capabilities {
        let all = self.inner.capabilities();
        match self.fault {
            Fault::UndeclaredLedger => Capabilities {
                ledger: Keeping::None,
                ..all
            },
            Fault::UndeclaredCursors => Capabilities {
                cursors: false,
                ..all
            },
            Fault::UndeclaredSchedules => Capabilities {
                schedules: Keeping::None,
                ..all
            },
            Fault::DerivesButKeeps | Fault::DerivesAtOneVersion => Capabilities {
                ledger: Keeping::Derived,
                ..all
            },
            Fault::DerivesSchedulesButKeeps => Capabilities {
                schedules: Keeping::Derived,
                ..all
            },
            Fault::Derives => Capabilities {
                ledger: Keeping::Derived,
                schedules: Keeping::Derived,
                ..all
            },
            Fault::UndeclaredSubscriptions => Capabilities {
                subscriptions: false,
                ..all
            },
            Fault::UndeclaredPins => Capabilities { pins: false, ..all },
            Fault::UndeclaredDeliveries => Capabilities {
                deliveries: false,
                ..all
            },
            Fault::UndeclaredNotes => Capabilities {
                notes: false,
                ..all
            },
            Fault::UndeclaredLatest => Capabilities {
                latest: false,
                ..all
            },
            Fault::KeepsNothing => Capabilities {
                ledger: Keeping::None,
                cursors: false,
                schedules: Keeping::None,
                subscriptions: false,
                pins: false,
                deliveries: false,
                notes: false,
                latest: false,
                concurrent: false,
            },
            _ => all,
        }
    }

    async fn entry(&self, key: &Key) -> Result<Option<Versioned<Entry>>, StoreError> {
        self.refuses("ledger")?;
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
        self.refuses("ledger")?;
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

    async fn cursor(&self, source: &SourceName) -> Result<Option<Versioned<Cursor>>, StoreError> {
        self.refuses("cursors")?;
        self.inner.cursor(source).await
    }

    async fn put_cursor(
        &self,
        source: &SourceName,
        cursor: &Cursor,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.refuses("cursors")?;
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

    async fn schedules(&self) -> Result<Vec<Schedule>, StoreError> {
        self.refuses("schedules")?;
        if self.fault == Fault::Derives {
            return Ok(Vec::new());
        }
        let held = self.inner.schedules().await?;
        Ok(match self.fault {
            Fault::DuplicatesSchedules => held.iter().chain(&held).cloned().collect(),
            _ => held,
        })
    }

    async fn put_schedules(&self, add: &[Schedule], remove: &[Schedule]) -> Result<(), StoreError> {
        self.refuses("schedules")?;
        self.inner.put_schedules(add, remove).await
    }

    async fn subscribers(
        &self,
        destination: &DestinationName,
        project: &str,
    ) -> Result<Vec<Subscription>, StoreError> {
        self.refuses("subscriptions")?;
        let mut held = self.inner.subscribers(destination, project).await?;
        if self.fault == Fault::IgnoresProjects {
            held.extend(self.inner.subscribers(destination, "other").await?);
        }
        Ok(held)
    }

    async fn subscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        self.refuses("subscriptions")?;
        self.inner.subscribe(subscription).await
    }

    async fn unsubscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        self.refuses("subscriptions")?;
        self.inner.unsubscribe(subscription).await
    }

    async fn pinned(&self, route: &RouteId) -> Result<Option<String>, StoreError> {
        self.refuses("pins")?;
        let route = match self.fault {
            Fault::SharesPins => RouteId::new("shared"),
            _ => route.clone(),
        };
        self.inner.pinned(&route).await
    }

    async fn put_pinned(&self, route: &RouteId, post: &str) -> Result<(), StoreError> {
        self.refuses("pins")?;
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

    async fn admit(&self, scheme: Scheme, id: &str, at: Timestamp) -> Result<Admitted, StoreError> {
        self.refuses("deliveries")?;
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
        self.refuses("deliveries")?;
        if self.fault == Fault::KeepsReleased {
            return Ok(());
        }
        self.inner.release(scheme, id).await
    }

    async fn expire_deliveries(&self, before: Timestamp) -> Result<(), StoreError> {
        self.refuses("deliveries")?;
        match self.fault {
            Fault::ExpiresNothing => Ok(()),
            Fault::ExpiresEverything => self.inner.expire_deliveries(Timestamp::MAX).await,
            _ => self.inner.expire_deliveries(before).await,
        }
    }

    async fn note(&self, key: &Key) -> Result<Option<String>, StoreError> {
        self.refuses("notes")?;
        let key = match self.fault {
            Fault::SharesNotes => super::key("shared"),
            _ => *key,
        };
        self.inner.note(&key).await
    }

    async fn put_note(&self, key: &Key, note: &str) -> Result<(), StoreError> {
        self.refuses("notes")?;
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

    async fn latest(&self, project: &str) -> Result<Option<Versioned<Latest>>, StoreError> {
        self.refuses("latest")?;
        self.inner.latest(project).await
    }

    async fn every_latest(&self) -> Result<Vec<Latest>, StoreError> {
        self.refuses("latest")?;
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
        self.refuses("latest")?;
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
fn a_kind_kept_but_not_declared_is_refused() {
    let cases = [
        (Fault::UndeclaredLedger, "ledger"),
        (Fault::UndeclaredCursors, "cursors"),
        (Fault::UndeclaredSchedules, "schedules"),
        (Fault::UndeclaredSubscriptions, "subscriptions"),
        (Fault::UndeclaredPins, "pins"),
        (Fault::UndeclaredDeliveries, "deliveries"),
        (Fault::UndeclaredNotes, "notes"),
        (Fault::UndeclaredLatest, "latest"),
    ];
    for (fault, what) in cases {
        assert_eq!(
            run(fault),
            Err(Failure(format!(
                "a store that does not declare {what} must answer unsupported"
            )))
        );
    }
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
