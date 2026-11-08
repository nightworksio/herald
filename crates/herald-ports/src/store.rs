use std::fmt;
use std::future::Future;

use herald_core::{
    Cursor, DestinationName, Entry, Key, Latest, RouteId, Schedule, Scheme, SourceName, State,
    Subscription,
};
use jiff::Timestamp;

/// The version a store keeps beside a record, for compare-and-set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(pub u64);

/// A record and its version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Versioned<T> {
    /// The record.
    pub value: T,
    /// Its version.
    pub version: Version,
}

/// What a compare-and-set did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written {
    /// It wrote, and the record now has this version.
    Written(Version),
    /// The record was not at the expected version, and nothing was written.
    Conflict,
}

/// How a store holds a kind of record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keeping {
    /// It keeps what is written, and reads it back.
    Kept,
    /// It reads them from the destinations' own posts, and takes each write
    /// without keeping it, as the post is the record (ADR-0005).
    Derived,
}

/// What admitting a delivery came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admitted {
    /// It is the first time the delivery arrives, and its id is now kept.
    Fresh,
    /// The delivery was admitted before.
    Seen,
}

/// Why a store could not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The store could not be reached.
    Unavailable {
        /// What went wrong.
        reason: String,
    },
    /// A record could not be read back.
    Corrupt {
        /// What went wrong.
        reason: String,
    },
    /// The store does not keep this kind of record.
    Unsupported {
        /// What it does not keep.
        what: &'static str,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable { reason } => write!(f, "the store could not be reached: {reason}"),
            Self::Corrupt { reason } => {
                write!(f, "a record in the store could not be read: {reason}")
            }
            Self::Unsupported { what } => {
                write!(f, "this store keeps no {what}; choose a store that does")
            }
        }
    }
}

/// A kind of record a store may keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The ledger.
    Ledger,
    /// Cursors.
    Cursors,
    /// Schedules.
    Schedules,
    /// Subscriptions.
    Subscriptions,
    /// Each route's pinned post.
    Pins,
    /// The ids of webhook deliveries it admitted.
    Deliveries,
    /// The Notes herald's ActivityPub actors published.
    Notes,
    /// What each project last announced.
    Latest,
    /// Where each delivery to a follower's inbox stands.
    Inboxes,
}

impl Kind {
    /// The kind as an error names it.
    const fn what(self) -> &'static str {
        match self {
            Self::Ledger => "ledger",
            Self::Cursors => "cursors",
            Self::Schedules => "schedules",
            Self::Subscriptions => "subscriptions",
            Self::Pins => "pins",
            Self::Deliveries => "deliveries",
            Self::Notes => "notes",
            Self::Latest => "latest",
            Self::Inboxes => "deliveries to followers",
        }
    }

    /// The error a store keeping none of this kind answers.
    const fn unsupported(self) -> StoreError {
        StoreError::Unsupported { what: self.what() }
    }
}

/// The ledger (ADR-0005). Entries are written with compare-and-set.
pub trait Ledger: Sync {
    /// Whether it keeps entries or derives them from the destinations'
    /// posts.
    fn keeping(&self) -> Keeping {
        Keeping::Kept
    }

    /// The ledger's entry for `key`.
    fn entry(
        &self,
        key: &Key,
    ) -> impl Future<Output = Result<Option<Versioned<Entry>>, StoreError>> + Send;

    /// Writes `entry` if the stored one is at `expected`, or absent when
    /// `expected` is `None`.
    fn put_entry(
        &self,
        entry: &Entry,
        expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send;
}

/// Each source's cursor, written with compare-and-set.
pub trait Cursors: Sync {
    /// The cursor of `source`.
    fn cursor(
        &self,
        source: &SourceName,
    ) -> impl Future<Output = Result<Option<Versioned<Cursor>>, StoreError>> + Send;

    /// Writes the cursor of `source` if the stored one is at `expected`, or
    /// absent when `expected` is `None`.
    fn put_cursor(
        &self,
        source: &SourceName,
        cursor: &Cursor,
        expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send;
}

/// The schedules (ADR-0011), a set: adding or removing one twice is the
/// same as once.
pub trait Schedules: Sync {
    /// Whether it keeps schedules or derives them from the destinations'
    /// posts.
    fn keeping(&self) -> Keeping {
        Keeping::Kept
    }

    /// Every schedule.
    fn every_schedule(&self) -> impl Future<Output = Result<Vec<Schedule>, StoreError>> + Send;

    /// Adds `add` and removes `remove`.
    fn put_schedules(
        &self,
        add: &[Schedule],
        remove: &[Schedule],
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

/// The subscriptions, a set: adding or removing one twice is the same as
/// once.
pub trait Subscriptions: Sync {
    /// The subscriptions to `project` on `destination`.
    fn subscribers(
        &self,
        destination: &DestinationName,
        project: &str,
    ) -> impl Future<Output = Result<Vec<Subscription>, StoreError>> + Send;

    /// Adds `subscription`.
    fn subscribe(
        &self,
        subscription: &Subscription,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Removes `subscription`.
    fn unsubscribe(
        &self,
        subscription: &Subscription,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

/// Each route's pinned post, one value: the last written is kept.
pub trait Pins: Sync {
    /// The post `route` last pinned.
    fn pinned(
        &self,
        route: &RouteId,
    ) -> impl Future<Output = Result<Option<String>, StoreError>> + Send;

    /// Keeps `post` as the post `route` last pinned.
    fn put_pinned(
        &self,
        route: &RouteId,
        post: &str,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

/// The ids of the webhook deliveries it admitted (ADR-0013).
pub trait Deliveries: Sync {
    /// Admits the delivery `id` of `scheme`, arriving `at`: fresh the first
    /// time, and seen every later time until it is released or expires. Two
    /// admitting the same delivery at once admit one.
    fn admit(
        &self,
        scheme: Scheme,
        id: &str,
        at: Timestamp,
    ) -> impl Future<Output = Result<Admitted, StoreError>> + Send;

    /// Forgets the delivery `id` of `scheme`, so it is admitted again.
    fn release(
        &self,
        scheme: Scheme,
        id: &str,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Forgets every delivery admitted before `before`.
    fn expire_deliveries(
        &self,
        before: Timestamp,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

/// The Notes herald's ActivityPub actors published, served by their id.
pub trait Notes: Sync {
    /// The Note the announcement `key` was published as, as its JSON.
    fn note(&self, key: &Key) -> impl Future<Output = Result<Option<String>, StoreError>> + Send;

    /// Keeps `note` as the Note the announcement `key` was published as.
    fn put_note(
        &self,
        key: &Key,
        note: &str,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

/// What each project last announced, for commands to answer with.
pub trait Announcements: Sync {
    /// What `project` last announced, with its version.
    fn latest(
        &self,
        project: &str,
    ) -> impl Future<Output = Result<Option<Versioned<Latest>>, StoreError>> + Send;

    /// What every project last announced.
    fn every_latest(&self) -> impl Future<Output = Result<Vec<Latest>, StoreError>> + Send;

    /// Writes `latest` if the project's record is still at `expected`, or
    /// absent where `expected` is `None`.
    fn put_latest(
        &self,
        latest: &Latest,
        expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send;
}

/// Where each delivery of an announcement to a follower's inbox stands
/// (ADR-0005), kept apart from the announcement's ledger entry so the entry
/// does not grow with its followers. Only the run holding the entry's lease
/// writes them, so the last write is kept.
pub trait Inboxes: Sync {
    /// Where the delivery of the announcement `key` to `inbox` stands.
    fn delivery(
        &self,
        key: &Key,
        inbox: &str,
    ) -> impl Future<Output = Result<Option<State>, StoreError>> + Send;

    /// Keeps `state` as where the delivery of `key` to `inbox` stands.
    fn put_delivery(
        &self,
        key: &Key,
        inbox: &str,
        state: &State,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

impl<T: Ledger> Ledger for Option<&T> {
    fn keeping(&self) -> Keeping {
        self.map_or(Keeping::Kept, Ledger::keeping)
    }

    async fn entry(&self, key: &Key) -> Result<Option<Versioned<Entry>>, StoreError> {
        match self {
            Some(kept) => kept.entry(key).await,
            None => Err(Kind::Ledger.unsupported()),
        }
    }

    async fn put_entry(
        &self,
        entry: &Entry,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_entry(entry, expected).await,
            None => Err(Kind::Ledger.unsupported()),
        }
    }
}

impl<T: Cursors> Cursors for Option<&T> {
    async fn cursor(&self, source: &SourceName) -> Result<Option<Versioned<Cursor>>, StoreError> {
        match self {
            Some(kept) => kept.cursor(source).await,
            None => Err(Kind::Cursors.unsupported()),
        }
    }

    async fn put_cursor(
        &self,
        source: &SourceName,
        cursor: &Cursor,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_cursor(source, cursor, expected).await,
            None => Err(Kind::Cursors.unsupported()),
        }
    }
}

impl<T: Schedules> Schedules for Option<&T> {
    fn keeping(&self) -> Keeping {
        self.map_or(Keeping::Kept, Schedules::keeping)
    }

    async fn every_schedule(&self) -> Result<Vec<Schedule>, StoreError> {
        match self {
            Some(kept) => kept.every_schedule().await,
            None => Err(Kind::Schedules.unsupported()),
        }
    }

    async fn put_schedules(&self, add: &[Schedule], remove: &[Schedule]) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.put_schedules(add, remove).await,
            None => Err(Kind::Schedules.unsupported()),
        }
    }
}

impl<T: Subscriptions> Subscriptions for Option<&T> {
    async fn subscribers(
        &self,
        destination: &DestinationName,
        project: &str,
    ) -> Result<Vec<Subscription>, StoreError> {
        match self {
            Some(kept) => kept.subscribers(destination, project).await,
            None => Err(Kind::Subscriptions.unsupported()),
        }
    }

    async fn subscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.subscribe(subscription).await,
            None => Err(Kind::Subscriptions.unsupported()),
        }
    }

    async fn unsubscribe(&self, subscription: &Subscription) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.unsubscribe(subscription).await,
            None => Err(Kind::Subscriptions.unsupported()),
        }
    }
}

impl<T: Pins> Pins for Option<&T> {
    async fn pinned(&self, route: &RouteId) -> Result<Option<String>, StoreError> {
        match self {
            Some(kept) => kept.pinned(route).await,
            None => Err(Kind::Pins.unsupported()),
        }
    }

    async fn put_pinned(&self, route: &RouteId, post: &str) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.put_pinned(route, post).await,
            None => Err(Kind::Pins.unsupported()),
        }
    }
}

impl<T: Deliveries> Deliveries for Option<&T> {
    async fn admit(&self, scheme: Scheme, id: &str, at: Timestamp) -> Result<Admitted, StoreError> {
        match self {
            Some(kept) => kept.admit(scheme, id, at).await,
            None => Err(Kind::Deliveries.unsupported()),
        }
    }

    async fn release(&self, scheme: Scheme, id: &str) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.release(scheme, id).await,
            None => Err(Kind::Deliveries.unsupported()),
        }
    }

    async fn expire_deliveries(&self, before: Timestamp) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.expire_deliveries(before).await,
            None => Err(Kind::Deliveries.unsupported()),
        }
    }
}

impl<T: Notes> Notes for Option<&T> {
    async fn note(&self, key: &Key) -> Result<Option<String>, StoreError> {
        match self {
            Some(kept) => kept.note(key).await,
            None => Err(Kind::Notes.unsupported()),
        }
    }

    async fn put_note(&self, key: &Key, note: &str) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.put_note(key, note).await,
            None => Err(Kind::Notes.unsupported()),
        }
    }
}

impl<T: Announcements> Announcements for Option<&T> {
    async fn latest(&self, project: &str) -> Result<Option<Versioned<Latest>>, StoreError> {
        match self {
            Some(kept) => kept.latest(project).await,
            None => Err(Kind::Latest.unsupported()),
        }
    }

    async fn every_latest(&self) -> Result<Vec<Latest>, StoreError> {
        match self {
            Some(kept) => kept.every_latest().await,
            None => Err(Kind::Latest.unsupported()),
        }
    }

    async fn put_latest(
        &self,
        latest: &Latest,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_latest(latest, expected).await,
            None => Err(Kind::Latest.unsupported()),
        }
    }
}

impl<T: Inboxes> Inboxes for Option<&T> {
    async fn delivery(&self, key: &Key, inbox: &str) -> Result<Option<State>, StoreError> {
        match self {
            Some(kept) => kept.delivery(key, inbox).await,
            None => Err(Kind::Inboxes.unsupported()),
        }
    }

    async fn put_delivery(&self, key: &Key, inbox: &str, state: &State) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.put_delivery(key, inbox, state).await,
            None => Err(Kind::Inboxes.unsupported()),
        }
    }
}

/// The records of a kind a store never keeps: there are none to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unkept {}

impl Ledger for Unkept {
    fn entry(
        &self,
        _: &Key,
    ) -> impl Future<Output = Result<Option<Versioned<Entry>>, StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }

    fn put_entry(
        &self,
        _: &Entry,
        _: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }
}

impl Cursors for Unkept {
    fn cursor(
        &self,
        _: &SourceName,
    ) -> impl Future<Output = Result<Option<Versioned<Cursor>>, StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }

    fn put_cursor(
        &self,
        _: &SourceName,
        _: &Cursor,
        _: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }
}

impl Schedules for Unkept {
    fn every_schedule(&self) -> impl Future<Output = Result<Vec<Schedule>, StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }

    fn put_schedules(
        &self,
        _: &[Schedule],
        _: &[Schedule],
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }
}

impl Subscriptions for Unkept {
    fn subscribers(
        &self,
        _: &DestinationName,
        _: &str,
    ) -> impl Future<Output = Result<Vec<Subscription>, StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }

    fn subscribe(&self, _: &Subscription) -> impl Future<Output = Result<(), StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }

    fn unsubscribe(&self, _: &Subscription) -> impl Future<Output = Result<(), StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }
}

impl Pins for Unkept {
    fn pinned(
        &self,
        _: &RouteId,
    ) -> impl Future<Output = Result<Option<String>, StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }

    fn put_pinned(
        &self,
        _: &RouteId,
        _: &str,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }
}

impl Deliveries for Unkept {
    fn admit(
        &self,
        _: Scheme,
        _: &str,
        _: Timestamp,
    ) -> impl Future<Output = Result<Admitted, StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }

    fn release(&self, _: Scheme, _: &str) -> impl Future<Output = Result<(), StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }

    fn expire_deliveries(
        &self,
        _: Timestamp,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }
}

impl Notes for Unkept {
    fn note(&self, _: &Key) -> impl Future<Output = Result<Option<String>, StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }

    fn put_note(&self, _: &Key, _: &str) -> impl Future<Output = Result<(), StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }
}

impl Announcements for Unkept {
    fn latest(
        &self,
        _: &str,
    ) -> impl Future<Output = Result<Option<Versioned<Latest>>, StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }

    fn every_latest(&self) -> impl Future<Output = Result<Vec<Latest>, StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }

    fn put_latest(
        &self,
        _: &Latest,
        _: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }
}

impl Inboxes for Unkept {
    fn delivery(
        &self,
        _: &Key,
        _: &str,
    ) -> impl Future<Output = Result<Option<State>, StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }

    fn put_delivery(
        &self,
        _: &Key,
        _: &str,
        _: &State,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        let never = *self;
        async move { match never {} }
    }
}

/// Where herald keeps its records (ADR-0005, ADR-0011): each kind it keeps,
/// through the narrow trait for that kind, and [`Unkept`] for each kind it
/// never keeps. What it keeps is read from which kinds it answers.
pub trait Store: Sync {
    /// Its ledger.
    type Ledger: Ledger;
    /// Its cursors.
    type Cursors: Cursors;
    /// Its schedules.
    type Schedules: Schedules;
    /// Its subscriptions.
    type Subscriptions: Subscriptions;
    /// Its pins.
    type Pins: Pins;
    /// Its admitted webhook deliveries.
    type Deliveries: Deliveries;
    /// Its Notes.
    type Notes: Notes;
    /// What its projects last announced.
    type Announcements: Announcements;
    /// Its deliveries to followers' inboxes.
    type Inboxes: Inboxes;

    /// The ledger, where it keeps or derives one.
    fn ledger(&self) -> Option<&Self::Ledger>;
    /// The cursors, where it keeps them.
    fn cursors(&self) -> Option<&Self::Cursors>;
    /// The schedules, where it keeps or derives them.
    fn schedules(&self) -> Option<&Self::Schedules>;
    /// The subscriptions, where it keeps them.
    fn subscriptions(&self) -> Option<&Self::Subscriptions>;
    /// The pins, where it keeps them.
    fn pins(&self) -> Option<&Self::Pins>;
    /// The admitted webhook deliveries, where it keeps them.
    fn deliveries(&self) -> Option<&Self::Deliveries>;
    /// The Notes, where it keeps them.
    fn notes(&self) -> Option<&Self::Notes>;
    /// What projects last announced, where it keeps it.
    fn announcements(&self) -> Option<&Self::Announcements>;
    /// The deliveries to followers' inboxes, where it keeps them.
    fn inboxes(&self) -> Option<&Self::Inboxes>;

    /// The paths of the records a listing skipped as corrupt since this was
    /// last asked, each once, so one bad record stops no listing and is
    /// still reported.
    fn skipped(&self) -> Vec<String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Derived;

    impl Ledger for Derived {
        fn keeping(&self) -> Keeping {
            Keeping::Derived
        }

        fn entry(
            &self,
            _: &Key,
        ) -> impl Future<Output = Result<Option<Versioned<Entry>>, StoreError>> + Send {
            std::future::ready(Ok(None))
        }

        fn put_entry(
            &self,
            _: &Entry,
            _: Option<Version>,
        ) -> impl Future<Output = Result<Written, StoreError>> + Send {
            std::future::ready(Ok(Written::Written(Version(7))))
        }
    }

    impl Schedules for Derived {
        fn keeping(&self) -> Keeping {
            Keeping::Derived
        }

        fn every_schedule(&self) -> impl Future<Output = Result<Vec<Schedule>, StoreError>> + Send {
            std::future::ready(Ok(Vec::new()))
        }

        fn put_schedules(
            &self,
            _: &[Schedule],
            _: &[Schedule],
        ) -> impl Future<Output = Result<(), StoreError>> + Send {
            std::future::ready(Ok(()))
        }
    }

    fn unsupported<T: fmt::Debug>(
        answer: impl Future<Output = Result<T, StoreError>>,
    ) -> &'static str {
        match pollster::block_on(answer) {
            Err(StoreError::Unsupported { what }) => what,
            other => panic!("answered {other:?}"),
        }
    }

    #[test]
    fn a_kind_kept_somewhere_answers_as_its_keeper_does() {
        let kept = Some(&Derived);
        assert_eq!(Ledger::keeping(&kept), Keeping::Derived);
        assert_eq!(Schedules::keeping(&kept), Keeping::Derived);
        let key = Key::derive(&RouteId::new("r"), &herald_core::Identity::new("1"));
        assert_eq!(pollster::block_on(kept.entry(&key)), Ok(None));
        assert_eq!(
            pollster::block_on(kept.put_entry(&Entry::new(key), None)),
            Ok(Written::Written(Version(7)))
        );
        assert_eq!(pollster::block_on(kept.every_schedule()), Ok(Vec::new()));
        assert_eq!(pollster::block_on(kept.put_schedules(&[], &[])), Ok(()));
    }

    #[test]
    fn the_run_s_kinds_kept_nowhere_answer_unsupported_naming_them() {
        let none = None::<&Unkept>;
        let key = Key::derive(&RouteId::new("r"), &herald_core::Identity::new("1"));
        let source = SourceName::new("s");
        let route = RouteId::new("r");
        let destination = DestinationName::new("d");
        let subscription = Subscription {
            destination: destination.clone(),
            project: "p".to_owned(),
            subscriber: "s".to_owned(),
            inbox: None,
        };
        assert_eq!(unsupported(none.entry(&key)), "ledger");
        assert_eq!(
            unsupported(none.put_entry(&Entry::new(key), None)),
            "ledger"
        );
        assert_eq!(unsupported(none.cursor(&source)), "cursors");
        assert_eq!(
            unsupported(none.put_cursor(&source, &Cursor::default(), None)),
            "cursors"
        );
        assert_eq!(unsupported(none.every_schedule()), "schedules");
        assert_eq!(unsupported(none.put_schedules(&[], &[])), "schedules");
        assert_eq!(
            unsupported(none.subscribers(&destination, "p")),
            "subscriptions"
        );
        assert_eq!(unsupported(none.subscribe(&subscription)), "subscriptions");
        assert_eq!(
            unsupported(none.unsubscribe(&subscription)),
            "subscriptions"
        );
        assert_eq!(unsupported(none.pinned(&route)), "pins");
        assert_eq!(unsupported(none.put_pinned(&route, "1")), "pins");
    }

    #[test]
    fn the_served_kinds_kept_nowhere_answer_unsupported_naming_them() {
        let none = None::<&Unkept>;
        let key = Key::derive(&RouteId::new("r"), &herald_core::Identity::new("1"));
        let state = State::Attempted {
            at: Timestamp::UNIX_EPOCH,
        };
        let at = Timestamp::UNIX_EPOCH;
        assert_eq!(
            unsupported(none.admit(Scheme::GitHub, "1", at)),
            "deliveries"
        );
        assert_eq!(unsupported(none.release(Scheme::GitHub, "1")), "deliveries");
        assert_eq!(unsupported(none.expire_deliveries(at)), "deliveries");
        assert_eq!(unsupported(none.note(&key)), "notes");
        assert_eq!(unsupported(none.put_note(&key, "{}")), "notes");
        assert_eq!(unsupported(none.latest("p")), "latest");
        assert_eq!(unsupported(none.every_latest()), "latest");
        let latest = Latest {
            project: "p".to_owned(),
            newest: herald_core::Shown {
                kind: herald_core::Kind::Release,
                version: None,
                title: "t".to_owned(),
                body: String::new(),
                links: std::collections::BTreeMap::new(),
                prerelease: false,
                occurred: at,
            },
            stable: None,
            routes: std::collections::BTreeSet::new(),
        };
        assert_eq!(unsupported(none.put_latest(&latest, None)), "latest");
        assert_eq!(
            unsupported(none.delivery(&key, "i")),
            "deliveries to followers"
        );
        assert_eq!(
            unsupported(none.put_delivery(&key, "i", &state)),
            "deliveries to followers"
        );
        assert_eq!(Ledger::keeping(&none), Keeping::Kept);
    }

    #[test]
    fn every_store_error_reads_as_a_sentence() {
        assert_eq!(
            StoreError::Unavailable {
                reason: "down".to_owned()
            }
            .to_string(),
            "the store could not be reached: down"
        );
        assert_eq!(
            StoreError::Corrupt {
                reason: "bad json".to_owned()
            }
            .to_string(),
            "a record in the store could not be read: bad json"
        );
        assert_eq!(
            StoreError::Unsupported { what: "cursors" }.to_string(),
            "this store keeps no cursors; choose a store that does"
        );
    }
}
