use std::fmt;
use std::future::{Future, ready};
use std::ops::Deref;
use std::sync::Arc;

use herald_core::status;
use herald_core::{Announcement, Chore, Delivery, Entry, Key, Marker, Profile, Sent, Step};
use herald_ports::{
    Base, Boxed, DynHttp, DynMarkers, DynSigner, HttpError, MarkedThread, Markers, MarkersError,
    Response,
};
use jiff::Timestamp;

use crate::{Fed, Federates, Itself, Pushed};

/// What a step is signed with, where its destination signs its requests.
#[derive(Clone, Copy)]
pub struct Signing<'a> {
    /// The signer.
    pub signer: &'a dyn DynSigner,
    /// The time the signature names.
    pub now: Timestamp,
}

/// One route's destination, its credential resolved: what every kind of
/// destination is to the flows that announce through it.
pub trait Reach: fmt::Debug + Send + Sync + Itself {
    /// What it can do.
    fn profile(&self) -> Profile;

    /// What its `Http` may reach: the base URL of everything it sends, or
    /// any public address (ADR-0007).
    fn base(&self) -> Base<'_>;

    /// Sends `step` of `announcement` through `http`, signed as `signing`
    /// says where the destination signs its requests, and reads the answer.
    fn send<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        signing: Signing<'a>,
        step: &'a Step,
        announcement: &'a Announcement,
        delivery: &'a Delivery,
        entry: &'a Entry,
    ) -> Boxed<'a, Result<Sent, HttpError>>;

    /// Does `chore` to `thread` through `http`.
    fn housekeep<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        chore: Chore,
        thread: &'a str,
    ) -> Boxed<'a, Result<Sent, HttpError>>;

    /// Its own posts, read back through `http` for their markers.
    fn posts<'a>(&'a self, http: &'a dyn DynHttp) -> Box<dyn DynMarkers + 'a>;

    /// The actor whose followers each announcement is delivered to, where
    /// the destination delivers to followers.
    fn follows(&self) -> Option<&str> {
        None
    }

    /// The document `announcement` is published as, beside the actor whose
    /// outbox lists it, where the destination serves what it publishes.
    fn published(&self, _announcement: &Announcement) -> Option<(&str, String)> {
        None
    }

    /// Whether `follower` may be delivered to: every one, where the
    /// destination follows no policy.
    fn admits(&self, _follower: &str) -> bool {
        true
    }

    /// It as one of herald's own actors, where herald serves it rather than
    /// posting to it.
    fn federates(&self) -> Option<&dyn Federates> {
        None
    }

    /// It as a feed of herald's own, where herald publishes each event to
    /// a feed it serves rather than posting it.
    fn feeds(&self) -> Option<Fed<'_>> {
        None
    }

    /// It as a push to a peer's inbox, where herald pushes each event to a
    /// peer rather than posting it.
    fn pushes(&self) -> Option<Pushed<'_>> {
        None
    }

    /// Who may read the place it posts to right now, asked through `http`
    /// before a run's first advisories section is sent there: every kind
    /// decides it, so a new one cannot be sent a section unchecked
    /// (ADR-0067).
    fn audience<'a>(&'a self, http: &'a dyn DynHttp) -> Boxed<'a, Audience>;

    /// Whether an edit there leaves what a post showed before readable, as
    /// an edit history or the replaced events of a room's timeline do: a
    /// living post of a view that reads advisories is then deleted and
    /// posted anew there, never edited. Every kind decides it (ADR-0067).
    fn keeps_edits(&self) -> bool;

    /// Whether `answer`, a client error answered as JSON to a request it
    /// sent, is its platform's own error envelope, so the platform itself
    /// declined the request and made nothing, rather than a page something
    /// in front of it answered with, maybe after the platform kept what was
    /// sent. A first send's record is dropped only on the platform's own
    /// word, so every kind decides it (ADR-0067).
    fn declines(&self, answer: &Response) -> bool;

    /// What its platform says, asked through `http`, of its credential and
    /// of what a route delivering as `delivery`, its announcements
    /// `recorded` so, needs, with nothing posted (ADR-0002): nothing where
    /// it has no platform to ask.
    fn probe<'a>(
        &'a self,
        _http: &'a dyn DynHttp,
        _delivery: &'a Delivery,
        _recorded: Recorded,
    ) -> Boxed<'a, Vec<Probe>> {
        Box::pin(ready(Vec::new()))
    }
}

/// Who may read the place a route posts to, as its destination answered
/// right before an advisories section would be sent there (ADR-0067).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Audience {
    /// Only those invited or let in may: herald checked it.
    Private,
    /// More may, for the reason given.
    Public(String),
    /// Herald could not tell, for the reason given.
    Unread(String),
}

impl Audience {
    /// Why an advisories section is withheld there; none where herald
    /// checked that only those invited or let in may read it.
    #[must_use]
    pub fn withheld(&self) -> Option<&str> {
        match self {
            Self::Private => None,
            Self::Public(why) | Self::Unread(why) => Some(why),
        }
    }
}

/// Why a kind that posts where anyone may read, or carries no view, is
/// never sent an advisories section, whatever a route says.
pub const UNADVISED: &str = "its kind of destination is never sent an advisories section";

/// Where a run finds what a route already announced (ADR-0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Recorded {
    /// In a store that keeps records.
    #[default]
    Kept,
    /// In the route's own posts, read back for their markers.
    Posts,
}

/// What a platform answers when asked whether it serves a route
/// (ADR-0002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// It takes the credential.
    Taken,
    /// It does not take the credential: it refuses it, or could not be
    /// asked.
    Credential(String),
    /// It withholds what the route needs, at the route's option `key` where
    /// one asks for it.
    Route {
        /// The route's option that asks for it.
        key: Option<&'static str>,
        /// What it withholds, and what would have it given.
        why: String,
    },
    /// What it says that the route could follow.
    Said(String),
    /// It addresses the route's place to anyone, for the reason given, so
    /// the route cannot say `private = true` (ADR-0067).
    Public(String),
}

impl Probe {
    /// What `platform`'s `answer` to a request made with the credential
    /// says of it.
    #[must_use]
    pub fn credential(platform: &str, answer: &Result<Response, HttpError>) -> Self {
        let why = match answer {
            Ok(response) if response.succeeded() => return Self::Taken,
            Ok(response) if response.refuses_credential() => format!(
                "{platform} refuses its credential, answering {}",
                response.status
            ),
            Ok(response) => format!(
                "{platform} answered {} when asked whether it takes its credential",
                response.status
            ),
            Err(error) => format!("{platform} could not be asked: {error}"),
        };
        Self::Credential(why)
    }

    /// What `platform`'s `answer` to a request for what a route needs says,
    /// at the route's option `key`: nothing where it gave it, and
    /// `withheld`, handed the status, where it says no.
    #[must_use]
    pub fn route(
        platform: &str,
        key: Option<&'static str>,
        answer: &Result<Response, HttpError>,
        withheld: &dyn Fn(u16) -> String,
    ) -> Option<Self> {
        let why = match answer {
            Ok(response) if response.succeeded() => return None,
            Ok(response)
                if response.refuses_credential() || response.status == status::NOT_FOUND =>
            {
                withheld(response.status)
            }
            Ok(response) => format!("{platform} answered {} when asked", response.status),
            Err(error) => format!("{platform} could not be asked: {error}"),
        };
        Some(Self::Route { key, why })
    }
}

/// A reached destination of any kind, shared by every route that reaches it.
#[derive(Clone)]
pub struct Reached(Arc<dyn Reach>);

impl Reached {
    /// `reach`, reached.
    #[must_use]
    pub fn new(reach: impl Reach) -> Self {
        Self(Arc::new(reach))
    }

    /// The destination as its own kind, where it is a `T`.
    #[must_use]
    pub fn of<T: Reach>(&self) -> Option<&T> {
        self.0.as_any().downcast_ref()
    }

    /// Its own posts, read back through `http` for their markers.
    #[must_use]
    pub fn read<'a>(&'a self, http: &'a dyn DynHttp) -> Posts<'a> {
        Posts(self.0.posts(http))
    }
}

impl Deref for Reached {
    type Target = dyn Reach;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl fmt::Debug for Reached {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl PartialEq for Reached {
    fn eq(&self, other: &Self) -> bool {
        self.0.same(other.0.as_any())
    }
}

impl Eq for Reached {}

/// One destination's own posts, read back for their markers.
pub struct Posts<'a>(pub Box<dyn DynMarkers + 'a>);

impl fmt::Debug for Posts<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Posts")
    }
}

impl Markers for Posts<'_> {
    async fn entry(&self, key: &Key, marker: &Marker) -> Result<Option<Entry>, MarkersError> {
        herald_ports::Erased(&*self.0).entry(key, marker).await
    }

    async fn threads(&self, route: &Marker) -> Result<Vec<MarkedThread>, MarkersError> {
        herald_ports::Erased(&*self.0).threads(route).await
    }
}

/// Posts a destination reads none of, and why: it serves what it publishes
/// rather than posting it somewhere it could read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnreadPosts(pub &'static str);

impl Markers for UnreadPosts {
    fn entry(
        &self,
        _: &Key,
        _: &Marker,
    ) -> impl Future<Output = Result<Option<Entry>, MarkersError>> + Send {
        ready(Err(self.unreadable()))
    }

    fn threads(
        &self,
        _: &Marker,
    ) -> impl Future<Output = Result<Vec<MarkedThread>, MarkersError>> + Send {
        ready(Err(self.unreadable()))
    }
}

impl UnreadPosts {
    fn unreadable(self) -> MarkersError {
        MarkersError::Unreadable {
            reason: self.0.to_owned(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
