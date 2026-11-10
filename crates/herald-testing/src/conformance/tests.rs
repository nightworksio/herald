use herald_core::State;
use herald_ports::{
    Admitted, Announcements, Cursors, Deliveries, Held, Inboxes, Keeper, Keeping, Keyring,
    KnownKey, Ledger, Notes, Outboxed, Pins, Schedules, Subscriptions, Versioned,
};
use jiff::Timestamp;

use super::*;
use crate::FakeStore;

/// A scheme as its crate writes it, for these tests.
const GITHUB: herald_core::Scheme = herald_core::Scheme {
    name: "github",
    signed: herald_core::Signed::Body {
        headers: &["X-Hub-Signature-256"],
        prefix: "sha256=",
    },
    key: herald_core::KeyForm::Bytes,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    OverwritesEntries,
    KeepsExpiredEntries,
    ExpiresEveryClaimedEntry,
    ExpiresUnclaimedEntries,
    AnswersNoExpiredKey,
    KeepsForgottenDeliveries,
    ForgetsEveryAnnouncement,
    KeepsExpiredNotes,
    ExpiresEveryNote,
    RefusesWrites,
    OverwritesCursors,
    DuplicatesSchedules,
    IgnoresProjects,
    KeepsTheFirstSubscription,
    ForgetsWhatAFollowAwaits,
    KeepsTheFirstInbox,
    ForgetsOnlyTheSameSubscription,
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
    ForgetsTheOutbox,
    NamesNoNextPage,
    NamesANextPageAtTheEnd,
    ListsTheOldestFirst,
    MiscountsTheOutbox,
    IgnoresWhereAPageEnded,
    SharesOutboxes,
    ListsARepublishedNoteTwice,
    KeepsExpiredNotesInTheOutbox,
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
    RacesEntries,
    RacesCursors,
    RacesLatest,
    RacesAdmissions,
}

struct Faulty {
    fault: Fault,
    inner: FakeStore,
    hidden: std::sync::Mutex<Vec<Key>>,
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
            hidden: std::sync::Mutex::default(),
        }
    }

    fn refuses(&self) -> Result<(), StoreError> {
        match self.fault {
            Fault::Broken => Err(broken()),
            _ => Ok(()),
        }
    }

    /// `written`, or written where it conflicted on a raced record and the
    /// store races `racing`: what a store whose check and write are apart
    /// answers every writer racing it.
    fn raced(&self, racing: Fault, raced: bool, written: Written) -> Written {
        if self.fault == racing && raced && written == Written::Conflict {
            Written::Written(Version(99))
        } else {
            written
        }
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

    async fn forget_deliveries(&self, key: &Key) -> Result<(), StoreError> {
        self.refuses()?;
        match self.fault {
            Fault::KeepsForgottenDeliveries => Ok(()),
            Fault::ForgetsEveryAnnouncement => {
                self.inner.forget_deliveries(key).await?;
                self.inner.forget_deliveries(&super::key("other")).await
            }
            _ => self.inner.forget_deliveries(key).await,
        }
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
        if self.fault == Fault::Derives || crate::lock(&self.hidden).contains(key) {
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
        let written = self.inner.put_entry(entry, expected).await?;
        Ok(self.raced(
            Fault::RacesEntries,
            entry.key == super::key("raced"),
            written,
        ))
    }

    async fn expire_entries(&self, before: Timestamp) -> Result<Vec<Key>, StoreError> {
        self.refuses()?;
        match self.fault {
            Fault::KeepsExpiredEntries => Ok(Vec::new()),
            Fault::ExpiresEveryClaimedEntry => self.inner.expire_entries(Timestamp::MAX).await,
            Fault::AnswersNoExpiredKey => {
                self.inner.expire_entries(before).await?;
                Ok(Vec::new())
            }
            Fault::ExpiresUnclaimedEntries => {
                crate::lock(&self.hidden).push(super::key("first"));
                self.inner.expire_entries(before).await
            }
            _ => self.inner.expire_entries(before).await,
        }
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
        let written = self.inner.put_cursor(source, cursor, expected).await?;
        Ok(self.raced(Fault::RacesCursors, source.as_str() == "raced", written))
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
        let held = self
            .inner
            .subscribers(&subscription.destination, &subscription.project)
            .await?;
        let earlier = held
            .into_iter()
            .find(|kept| kept.subscriber == subscription.subscriber);
        let subscription = match (self.fault, earlier) {
            (Fault::KeepsTheFirstSubscription, Some(_)) => return Ok(()),
            (Fault::KeepsTheFirstInbox, Some(earlier)) => Subscription {
                inbox: earlier.inbox,
                ..subscription.clone()
            },
            (Fault::ForgetsWhatAFollowAwaits, _) => Subscription {
                pending: None,
                ..subscription.clone()
            },
            _ => subscription.clone(),
        };
        self.inner.subscribe(&subscription).await
    }

    async fn unsubscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        self.refuses()?;
        if self.fault == Fault::ForgetsOnlyTheSameSubscription {
            let held = self
                .inner
                .subscribers(&subscription.destination, &subscription.project)
                .await?;
            if !held.contains(subscription) {
                return Ok(());
            }
        }
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
    async fn admit(
        &self,
        scheme: herald_core::Scheme,
        id: &str,
        at: Timestamp,
    ) -> Result<Admitted, StoreError> {
        self.refuses()?;
        let scheme = match self.fault {
            Fault::SharesDeliveries => GITHUB,
            _ => scheme,
        };
        let admitted = self.inner.admit(scheme, id, at).await?;
        Ok(match self.fault {
            Fault::AdmitsTwice => Admitted::Fresh,
            Fault::RacesAdmissions if id == "raced" => Admitted::Fresh,
            _ => admitted,
        })
    }

    async fn release(&self, scheme: herald_core::Scheme, id: &str) -> Result<(), StoreError> {
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

    async fn put_note(
        &self,
        actor: &str,
        key: &Key,
        note: &str,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        self.refuses()?;
        let key = match self.fault {
            Fault::SharesNotes => super::key("shared"),
            _ => *key,
        };
        let kept = self.inner.note(&key).await?;
        if self.fault == Fault::KeepsTheFirstNote && kept.is_some() {
            return Ok(());
        }
        let key = match (self.fault, kept) {
            (Fault::ListsARepublishedNoteTwice, Some(_)) if key == super::key("first") => {
                super::key("first again")
            }
            _ => key,
        };
        self.inner.put_note(actor, &key, note, at).await
    }

    async fn outbox(
        &self,
        actor: &str,
        after: Option<&str>,
        most: usize,
    ) -> Result<Outboxed, StoreError> {
        self.refuses()?;
        let actor = match self.fault {
            Fault::SharesOutboxes => "announcer",
            _ => actor,
        };
        let after = match self.fault {
            Fault::IgnoresWhereAPageEnded => None,
            _ => after,
        };
        let mut page = self.inner.outbox(actor, after, most).await?;
        match self.fault {
            Fault::ForgetsTheOutbox => return Ok(Outboxed::default()),
            Fault::NamesNoNextPage => page.next = None,
            Fault::NamesANextPageAtTheEnd => {
                page.next.get_or_insert_with(|| "end".to_owned());
            }
            Fault::ListsTheOldestFirst => page.notes.reverse(),
            Fault::MiscountsTheOutbox => page.total += 1,
            _ => {}
        }
        Ok(page)
    }

    async fn expire_notes(&self, before: Timestamp) -> Result<(), StoreError> {
        self.refuses()?;
        match self.fault {
            Fault::KeepsExpiredNotes => Ok(()),
            Fault::KeepsExpiredNotesInTheOutbox
                if before > NOON + jiff::SignedDuration::from_hours(1) =>
            {
                Ok(())
            }
            Fault::ExpiresEveryNote => self.inner.expire_notes(Timestamp::MAX).await,
            _ => self.inner.expire_notes(before).await,
        }
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
        let written = self.inner.put_latest(latest, expected).await?;
        Ok(self.raced(Fault::RacesLatest, latest.project == "raced", written))
    }
}

impl Keeper for Faulty {
    fn holds(&self, _: Held) -> bool {
        self.fault != Fault::KeepsNothing
    }

    fn skipped(&self) -> Vec<String> {
        if self.fault == Fault::SkipsWhatItNeverHeld {
            vec!["corrupt.json".to_owned()]
        } else {
            Keeper::skipped(&self.inner)
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

fn refused(cases: &[(Fault, &str)]) {
    for (fault, failure) in cases {
        assert_eq!(run(*fault), Err(Failure((*failure).to_owned())));
    }
}

#[test]
fn a_store_that_breaks_its_promises_on_the_run_s_records_is_refused() {
    refused(&[
        (
            Fault::SkipsWhatItNeverHeld,
            "a store holding no record skipped one",
        ),
        (
            Fault::OverwritesEntries,
            "an entry was written as new over one that exists",
        ),
        (
            Fault::KeepsExpiredEntries,
            "an entry no run has claimed since the expiry was kept",
        ),
        (
            Fault::ExpiresEveryClaimedEntry,
            "an entry claimed at the expiry was forgotten",
        ),
        (
            Fault::ExpiresUnclaimedEntries,
            "an entry no run ever claimed was forgotten",
        ),
        (
            Fault::AnswersNoExpiredKey,
            "expiring entries did not answer the key of each entry it forgot",
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
            Fault::KeepsTheFirstSubscription,
            "a subscriber subscribed again was not kept once, as subscribed last",
        ),
        (
            Fault::ForgetsWhatAFollowAwaits,
            "a subscriber subscribed again was not kept once, as subscribed last",
        ),
        (
            Fault::KeepsTheFirstInbox,
            "a subscriber subscribed again was not kept once, as subscribed last",
        ),
        (
            Fault::ForgetsOnlyTheSameSubscription,
            "a subscriber kept as subscribed otherwise was not forgotten",
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
            Fault::KeepsForgottenDeliveries,
            "a delivery of an announcement whose deliveries were forgotten was kept",
        ),
        (
            Fault::ForgetsEveryAnnouncement,
            "forgetting one announcement's deliveries forgot another's",
        ),
    ]);
}

#[test]
fn a_store_that_breaks_its_promises_on_the_served_records_is_refused() {
    refused(&[
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
            Fault::KeepsExpiredNotes,
            "a note published before the expiry was kept",
        ),
        (
            Fault::ExpiresEveryNote,
            "a note published at the expiry was forgotten",
        ),
        (
            Fault::ForgetsTheOutbox,
            "an actor's outbox did not list its newest notes first",
        ),
        (
            Fault::NamesNoNextPage,
            "an outbox with more notes than a page named no next page",
        ),
        (
            Fault::NamesANextPageAtTheEnd,
            "an outbox's next page did not hold the notes older than the last",
        ),
        (
            Fault::ListsTheOldestFirst,
            "an actor's outbox did not list its newest notes first",
        ),
        (
            Fault::MiscountsTheOutbox,
            "an actor's outbox did not list its newest notes first",
        ),
        (
            Fault::IgnoresWhereAPageEnded,
            "an outbox's next page did not hold the notes older than the last",
        ),
        (
            Fault::SharesOutboxes,
            "an actor's outbox listed another actor's notes",
        ),
        (
            Fault::ListsARepublishedNoteTwice,
            "a note published again was not listed once, as published last",
        ),
        (
            Fault::KeepsExpiredNotesInTheOutbox,
            "an expired note stayed in its actor's outbox",
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
    ]);
}

#[test]
fn a_store_whose_check_and_write_are_apart_is_refused() {
    refused(&[
        (
            Fault::RacesEntries,
            "of writers racing to make an entry, other than one wrote it",
        ),
        (
            Fault::RacesCursors,
            "of writers racing to make a cursor, other than one wrote it",
        ),
        (
            Fault::RacesLatest,
            "of writers racing to record what a project last announced, other than one wrote it",
        ),
        (
            Fault::RacesAdmissions,
            "of runs admitting one delivery at once, other than one admitted it fresh",
        ),
    ]);
}

#[test]
fn a_store_answering_a_kind_it_keeps_none_of_is_refused() {
    let store = FakeStore::default();
    let probed = |checks: Checks, method: &str| {
        assert_eq!(
            checks.done(),
            Err(Failure(format!(
                "{method} was not refused as unsupported by a store keeping none of its kind"
            )))
        );
    };
    let mut checks = Checks::default();
    pollster::block_on(super::unkept::ledger(&store, &mut checks));
    probed(checks, "entry");
    let mut checks = Checks::default();
    pollster::block_on(super::unkept::cursors(&store, &mut checks));
    probed(checks, "cursor");
    let mut checks = Checks::default();
    pollster::block_on(super::unkept::schedules(&store, &mut checks));
    probed(checks, "every_schedule");
    let mut checks = Checks::default();
    pollster::block_on(super::unkept::subscriptions(&store, &mut checks));
    probed(checks, "subscribers");
    let mut checks = Checks::default();
    pollster::block_on(super::unkept::pins(&store, &mut checks));
    probed(checks, "pinned");
    let mut checks = Checks::default();
    pollster::block_on(super::unkept::deliveries(&store, &mut checks));
    probed(checks, "admit");
    let mut checks = Checks::default();
    pollster::block_on(super::unkept::notes(&store, &mut checks));
    probed(checks, "note");
    let mut checks = Checks::default();
    pollster::block_on(super::unkept::announcements(&store, &mut checks));
    probed(checks, "latest");
    let mut checks = Checks::default();
    pollster::block_on(super::unkept::inboxes(&store, &mut checks));
    probed(checks, "delivery");
}

#[test]
fn every_method_of_a_kind_a_store_keeps_none_of_is_probed() {
    let mut checks = Checks::default();
    let unkept = Option::<&FakeStore>::None;
    pollster::block_on(async {
        super::unkept::ledger(&unkept, &mut checks).await;
        super::unkept::cursors(&unkept, &mut checks).await;
        super::unkept::schedules(&unkept, &mut checks).await;
        super::unkept::subscriptions(&unkept, &mut checks).await;
        super::unkept::pins(&unkept, &mut checks).await;
        super::unkept::deliveries(&unkept, &mut checks).await;
        super::unkept::notes(&unkept, &mut checks).await;
        super::unkept::announcements(&unkept, &mut checks).await;
        super::unkept::inboxes(&unkept, &mut checks).await;
    });
    assert_eq!(checks.done(), Ok(()));
    let mut checks = Checks::default();
    let store = FakeStore::default();
    pollster::block_on(super::unkept::notes(&store, &mut checks));
    assert_eq!(
        checks.0,
        [
            "note was not refused as unsupported by a store keeping none of its kind",
            "put_note was not refused as unsupported by a store keeping none of its kind",
            "outbox was not refused as unsupported by a store keeping none of its kind",
            "expire_notes was not refused as unsupported by a store keeping none of its kind",
        ]
    );
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
