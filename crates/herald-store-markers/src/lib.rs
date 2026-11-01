//! herald's ledger and thread schedules read from the destinations' own
//! posts (ADR-0005, ADR-0011).
//!
//! Each announcement's post carries its markers, so the post is the record:
//! the ledger entry for a key is what herald's own post carrying that key's
//! marker shows, and each thread opened from a post carrying a route's
//! marker falls due as that route's thread asks. Writes are taken and kept
//! nowhere. It keeps no cursors, subscriptions or pins, and two runs at once
//! are told apart only by the destination's own idempotency.

use std::future::{Future, ready};

use herald_core::{
    Chore, Cursor, DestinationName, Entry, Identity, Key, Marker, RouteId, Schedule, SourceName,
    Subscription, Thread, schedules,
};
use herald_ports::{
    Capabilities, Keeping, Markers, Secret, Signer, Store, StoreError, Version, Versioned, Written,
    marker,
};

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

impl<M: Markers, G: Signer> Store for MarkersStore<M, G> {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            ledger: Keeping::Derived,
            cursors: false,
            schedules: Keeping::Derived,
            subscriptions: false,
            pins: false,
            concurrent: false,
        }
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

    fn cursor(
        &self,
        _source: &SourceName,
    ) -> impl Future<Output = Result<Option<Versioned<Cursor>>, StoreError>> + Send {
        ready(unsupported("cursors"))
    }

    fn put_cursor(
        &self,
        _source: &SourceName,
        _cursor: &Cursor,
        _expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send {
        ready(unsupported("cursors"))
    }

    async fn schedules(&self) -> Result<Vec<Schedule>, StoreError> {
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

    fn subscribers(
        &self,
        _destination: &DestinationName,
        _project: &str,
    ) -> impl Future<Output = Result<Vec<String>, StoreError>> + Send {
        ready(unsupported("subscriptions"))
    }

    fn subscribe(
        &self,
        _subscription: &Subscription,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        ready(unsupported("subscriptions"))
    }

    fn unsubscribe(
        &self,
        _subscription: &Subscription,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        ready(unsupported("subscriptions"))
    }

    fn pinned(
        &self,
        _route: &RouteId,
    ) -> impl Future<Output = Result<Option<String>, StoreError>> + Send {
        ready(unsupported("pins"))
    }

    fn put_pinned(
        &self,
        _route: &RouteId,
        _post: &str,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        ready(unsupported("pins"))
    }
}
