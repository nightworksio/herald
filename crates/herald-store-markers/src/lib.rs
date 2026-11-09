//! herald's ledger and thread schedules read from the destinations' own
//! posts (ADR-0005, ADR-0011).
//!
//! Each announcement's post carries its markers, so the post is the record:
//! the ledger entry for a key is what herald's own post carrying that key's
//! marker shows, and each thread opened from a post carrying a route's
//! marker falls due as that route's thread asks. Writes are taken and kept
//! nowhere. It keeps no cursors, subscriptions or pins, and two runs at once
//! are told apart only by the destination's own idempotency.

mod kind;

pub use kind::{MarkersKind, MarkersOptions};

use std::future::{Future, ready};

use herald_core::{Chore, Entry, Identity, Key, Marker, RouteId, Schedule, Thread, schedules};
use herald_ports::{
    Keeping, Keyring, KnownKey, Ledger, Markers, Schedules, Secret, Signer, Store, StoreError,
    Unkept, Version, Versioned, Written, marker,
};
use jiff::Timestamp;

/// One route whose posts are read back: the markers it reads, and the key
/// its markers are computed under.
#[derive(Debug, Clone)]
pub struct MarkedRoute<M> {
    /// The route.
    pub route: RouteId,
    /// The thread its posts open, whose chores fall due.
    pub thread: Option<Thread>,
    /// Its destination's posts.
    pub markers: M,
    /// The key its markers are computed under.
    pub key: Secret,
}

/// The store the routes' own posts make.
#[derive(Debug, Clone)]
pub struct MarkersStore<M, G> {
    routes: Vec<MarkedRoute<M>>,
    signer: G,
}

fn unavailable(error: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable {
        reason: error.to_string(),
    }
}

fn unsupported<T>(what: &'static str) -> Result<T, StoreError> {
    Err(StoreError::Unsupported { what })
}

fn after(expected: Option<Version>) -> Written {
    Written::Written(Version(expected.map_or(1, |Version(number)| number + 1)))
}

impl<M: Markers, G: Signer> MarkersStore<M, G> {
    /// The store reading `routes`' posts, computing their markers by
    /// `signer`.
    #[must_use]
    pub fn new(routes: Vec<MarkedRoute<M>>, signer: G) -> Self {
        Self { routes, signer }
    }

    async fn mark(&self, key: &Secret, message: &[u8]) -> Result<Marker, StoreError> {
        marker(&self.signer, key, message)
            .await
            .map_err(unavailable)
    }

    async fn derived(
        &self,
        route: &MarkedRoute<M>,
        thread: &Thread,
    ) -> Result<Vec<Schedule>, StoreError> {
        let carried = self
            .mark(&route.key, &Marker::route_message(&route.route))
            .await?;
        let mut derived = Vec::new();
        for opened in route.markers.threads(&carried).await.map_err(unavailable)? {
            let key = Key::derive(
                &route.route,
                &Identity::new(format!("thread/{}", opened.thread)),
            );
            derived.extend(
                schedules(key, &route.route, thread, &opened.thread, opened.opened)
                    .into_iter()
                    .filter(|schedule| match schedule.chore {
                        Chore::Archive => !opened.archived,
                        Chore::Lock => !opened.locked,
                    }),
            );
        }
        Ok(derived)
    }
}

impl<M: Markers, G: Signer> Keyring for MarkersStore<M, G> {
    fn known(
        &self,
        _id: &str,
    ) -> impl Future<Output = Result<Option<KnownKey>, StoreError>> + Send {
        ready(unsupported("keys"))
    }

    fn know(
        &self,
        _id: &str,
        _key: &KnownKey,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        ready(unsupported("keys"))
    }

    fn forget_before(
        &self,
        _before: Timestamp,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        ready(unsupported("keys"))
    }
}

impl<M: Markers, G: Signer> Ledger for MarkersStore<M, G> {
    fn keeping(&self) -> Keeping {
        Keeping::Derived
    }

    async fn entry(&self, key: &Key) -> Result<Option<Versioned<Entry>>, StoreError> {
        for route in &self.routes {
            let carried = self.mark(&route.key, &Marker::message(key)).await?;
            if let Some(entry) = route
                .markers
                .entry(key, &carried)
                .await
                .map_err(unavailable)?
            {
                return Ok(Some(Versioned {
                    value: entry,
                    version: Version(1),
                }));
            }
        }
        Ok(None)
    }

    fn put_entry(
        &self,
        _entry: &Entry,
        expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send {
        ready(Ok(after(expected)))
    }

    fn expire_entries(
        &self,
        _before: Timestamp,
    ) -> impl Future<Output = Result<Vec<Key>, StoreError>> + Send {
        ready(Ok(Vec::new()))
    }
}

impl<M: Markers, G: Signer> Schedules for MarkersStore<M, G> {
    fn keeping(&self) -> Keeping {
        Keeping::Derived
    }

    async fn every_schedule(&self) -> Result<Vec<Schedule>, StoreError> {
        let mut derived = Vec::new();
        for route in &self.routes {
            if let Some(thread) = &route.thread {
                derived.extend(self.derived(route, thread).await?);
            }
        }
        Ok(derived)
    }

    fn put_schedules(
        &self,
        _add: &[Schedule],
        _remove: &[Schedule],
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        ready(Ok(()))
    }
}

impl<M: Markers, G: Signer> Store for MarkersStore<M, G> {
    type Ledger = Self;
    type Cursors = Unkept;
    type Schedules = Self;
    type Subscriptions = Unkept;
    type Pins = Unkept;
    type Deliveries = Unkept;
    type Notes = Unkept;
    type Announcements = Unkept;
    type Inboxes = Unkept;

    fn ledger(&self) -> Option<&Self> {
        Some(self)
    }

    fn cursors(&self) -> Option<&Unkept> {
        None
    }

    fn schedules(&self) -> Option<&Self> {
        Some(self)
    }

    fn subscriptions(&self) -> Option<&Unkept> {
        None
    }

    fn pins(&self) -> Option<&Unkept> {
        None
    }

    fn deliveries(&self) -> Option<&Unkept> {
        None
    }

    fn notes(&self) -> Option<&Unkept> {
        None
    }

    fn announcements(&self) -> Option<&Unkept> {
        None
    }

    fn inboxes(&self) -> Option<&Unkept> {
        None
    }

    fn skipped(&self) -> Vec<String> {
        vec![]
    }
}
