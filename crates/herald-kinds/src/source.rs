use std::any::Any;
use std::collections::BTreeSet;
use std::fmt;
use std::num::NonZeroUsize;
use std::ops::Deref;
use std::sync::Arc;

use herald_core::{
    Cursor, Event, EventsRead, Kind, Listed, Listing, Origin, Owner, Polled, Reach, RouteId,
    Scheme, SecretRef, Visibility,
};
use herald_ports::{Boxed, DynHttp, Secret};
use jiff::SignedDuration;

use crate::{Fault, Itself};

/// One source's table in `herald.toml`, read: what every kind of source's
/// options are to the config and the flows that read it.
pub trait SourceConfig: fmt::Debug + Send + Sync + Itself {
    /// The kind's name, as `[sources.<name>] kind` writes it.
    fn kind(&self) -> &'static str;

    /// The token it reads its platform with, where it names one.
    fn token(&self) -> Option<&SecretRef>;

    /// The secret its webhooks are signed with, where it takes them.
    fn webhook(&self) -> Option<&SecretRef> {
        None
    }

    /// Every secret it names.
    fn secret_refs(&self) -> Vec<&SecretRef> {
        self.token().into_iter().chain(self.webhook()).collect()
    }

    /// The kinds of event it produces.
    fn kinds(&self) -> &'static [Kind];

    /// How many of its newest events the first poll announces (ADR-0003).
    fn backfill(&self) -> usize;

    /// Who signs its webhooks, and how, where it takes them (ADR-0007).
    fn scheme(&self) -> Option<Scheme> {
        None
    }

    /// What the webhook delivery `payload` with `headers` asks herald to
    /// announce, where it takes webhooks.
    ///
    /// # Errors
    ///
    /// Why the delivery could not be read.
    fn hooked(&self, _headers: &[(&str, &str)], _payload: &[u8]) -> Result<Hooked, String> {
        Ok(Hooked::Nothing(UNHOOKED))
    }

    /// What the doctor says of every run that serves it, where its options
    /// leave something to its platform or to its polls.
    fn notes(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// Whether a name it announces can move to another version, as a
    /// registry's `latest` tag can.
    fn moving_tags(&self) -> bool {
        false
    }

    /// Whether it may take a repository not everyone may see.
    fn unlisted(&self) -> bool {
        false
    }

    /// Whether it may hold an event back until it is due, as a tag waiting
    /// for its Release (ADR-0032).
    fn holds(&self) -> bool {
        false
    }

    /// How long a tag of the one repository it names waits for its Release,
    /// where it names one and a tag waits (ADR-0032).
    fn tag_window(&self) -> Option<SignedDuration> {
        None
    }

    /// The signals its events may carry: a route taking them may see one
    /// announced again, changed, as a Release after its tag (ADR-0034).
    fn signals(&self) -> Vec<herald_core::Signal> {
        Vec::new()
    }

    /// The projects its events are announced as, where its options name
    /// them.
    fn projects(&self) -> Vec<String> {
        Vec::new()
    }

    /// Whether every event it gives carries an origin.
    fn origins(&self) -> bool {
        false
    }

    /// Each route its options name for some of its events beside the routes
    /// naming it, by id, with the key naming it under the source's table.
    fn routed(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Whether its options name the route `route` for some of its events.
    fn names_route(&self, route: &RouteId) -> bool {
        self.routed().iter().any(|(_, id)| id == route.as_str())
    }

    /// What polls it, reading with `token` where it has one, and watching
    /// what it announced for an edit where `edits` says a route edits its
    /// posts (ADR-0034).
    fn poller(&self, token: Option<Secret>, edits: bool) -> Polling;
}

/// How a source is polled: as one, or as each member an owner lists.
#[derive(Debug, Clone)]
pub enum Polling {
    /// One poll.
    One(Arc<dyn Poll>),
    /// An owner's members, each polled on its own.
    Many(Arc<dyn Expand>),
}

impl Polling {
    /// The base URL of everything it asks, the only origin its `Http` may
    /// reach (ADR-0007).
    #[must_use]
    pub fn api(&self) -> &str {
        match self {
            Self::One(poll) => poll.api(),
            Self::Many(expand) => expand.api(),
        }
    }
}

/// A source that expands to an owner's repositories, each polled on its
/// own.
pub trait Expand: fmt::Debug + Send + Sync {
    /// The base URL of everything it asks, the only origin its `Http` may
    /// reach (ADR-0007).
    fn api(&self) -> &str;

    /// How long a member that left the selection is kept before it is
    /// forgotten with its cursor.
    fn keep_members(&self) -> SignedDuration;

    /// How many identities each member's cursor remembers, newest first.
    fn remembered(&self) -> usize;

    /// The members the owner lists through `http` that the selection takes,
    /// asked conditionally on `known`, the last listing.
    fn listed<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        known: Option<&'a Listing>,
    ) -> Boxed<'a, Result<Expanded, Unlisted>>;

    /// What polls the member `repository`.
    fn member(&self, repository: &Listed) -> Arc<dyn Poll>;

    /// The members whose releases the owner's events name since `read`,
    /// read through `http` once a round, for an owner of the kind `owner`;
    /// nothing where the source polls every member each round.
    fn changed<'a>(
        &'a self,
        _http: &'a dyn DynHttp,
        _owner: Owner,
        _read: Option<&'a EventsRead>,
    ) -> Boxed<'a, Option<Result<Changed, Unpolled>>> {
        Box::pin(std::future::ready(None))
    }

    /// What the source reads through `http` once a round, before any
    /// member is polled, for an owner of the kind `owner`: its registry's
    /// packages, where its members announce them (ADR-0035); nothing where
    /// it reads nothing once a round.
    fn rounded<'a>(
        &'a self,
        _http: &'a dyn DynHttp,
        _owner: Owner,
    ) -> Boxed<'a, Result<Rounded, Unpolled>> {
        Box::pin(std::future::ready(Ok(Rounded::Nothing)))
    }

    /// How many members one request polls together; one where each is
    /// polled on its own.
    fn batch(&self) -> NonZeroUsize {
        NonZeroUsize::MIN
    }

    /// What each of `members`, each with its cursor, answers through
    /// `http`, in order, with what the round read once as `rounded`: polled
    /// together where the source batches them, and each through its own
    /// poll otherwise.
    fn polled<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        members: &'a [(&'a Listed, Option<&'a Cursor>)],
        _rounded: &'a Rounded,
    ) -> Boxed<'a, Vec<Result<Option<Polled>, Unpolled>>> {
        Box::pin(each_polled(self, http, members))
    }

    /// How long a tag the member `repository` publishes waits for its
    /// Release before it is announced, where a tag waits (ADR-0032).
    fn tag_window(&self, _repository: &Listed) -> Option<SignedDuration> {
        None
    }

    /// Where the member `repository`'s events come from.
    fn origin(&self, repository: &Listed) -> Origin;

    /// The routes the member `repository` names beside, or in place of,
    /// those naming the source.
    fn reach(&self, _repository: &Listed) -> Reach {
        Reach::default()
    }

    /// The project the member `repository`'s events are announced as.
    ///
    /// # Errors
    ///
    /// Why its project template names none.
    fn project(&self, repository: &Listed) -> Result<String, String>;

    /// Whether the selection takes `repository`, as the last listing named
    /// it.
    fn takes(&self, repository: &Listed) -> bool;

    /// A digest of what a listing asks for: the owner, where, and the
    /// selection.
    fn fingerprint(&self) -> String;
}

/// What each of `members`, each with its cursor, answers through `http`,
/// each through the poll `expand` gives it, in order.
pub async fn each_polled<E: Expand + ?Sized>(
    expand: &E,
    http: &dyn DynHttp,
    members: &[(&Listed, Option<&Cursor>)],
) -> Vec<Result<Option<Polled>, Unpolled>> {
    let mut answered = Vec::new();
    for (repository, cursor) in members {
        answered.push(expand.member(repository).polled(http, *cursor).await);
    }
    answered
}

/// What an owner source read once a round for every member, before any
/// was polled (ADR-0035).
#[derive(Debug, Clone, Default)]
pub enum Rounded {
    /// The source reads nothing once a round.
    #[default]
    Nothing,
    /// What it read.
    Read {
        /// The forge id of each member it names, polled this round whatever
        /// the owner's events say.
        named: BTreeSet<String>,
        /// What the source's own polls read of it.
        read: Arc<dyn Any + Send + Sync>,
    },
    /// Its read failed or was not sent, so each member's poll leaves out
    /// what it would have read.
    Unread,
}

impl Rounded {
    /// The forge id of each member it names.
    #[must_use]
    pub fn named(&self) -> Option<&BTreeSet<String>> {
        match self {
            Self::Read { named, .. } => Some(named),
            Self::Nothing | Self::Unread => None,
        }
    }
}

/// Which members an owner's events name since they were last read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changed {
    /// The events read reach back to where the last read ended: the forge
    /// id of each member they name a release of, and how far they were
    /// read; unchanged where `read` is the last one's.
    Named {
        /// The forge id of each member named.
        ids: BTreeSet<String>,
        /// How far the events were read.
        read: EventsRead,
    },
    /// The events read do not reach back to where the last read ended, or
    /// there was no last read, so every member is polled; how far they
    /// were read.
    Gap(EventsRead),
}

/// What listing an owner found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expanded {
    /// Nothing changed since the last listing.
    Unchanged,
    /// The listing read in full.
    Listed {
        /// The repositories the selection takes.
        members: Vec<Listed>,
        /// What kind of account the owner is.
        owner: Owner,
        /// The `ETag` of each page read, in order.
        pages: Vec<Option<String>>,
        /// The `ETag` of the member file, where it was read through the
        /// forge.
        file: Option<String>,
        /// The names the member file lists that the owner does not hold.
        missing: Vec<String>,
    },
}

impl Expanded {
    /// The members the listing takes; none where it was unchanged.
    #[must_use]
    pub fn members(self) -> Vec<Listed> {
        match self {
            Self::Unchanged => Vec::new(),
            Self::Listed { members, .. } => members,
        }
    }
}

/// Why listing an owner found no members to take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unlisted {
    /// The owner lists more than a setting allows, as this says, naming
    /// the setting; the source is refused for the round.
    Over(String),
    /// The listing failed, or failed partway, for this reason.
    Failed(Unpolled),
}

/// Why a source that takes no webhooks announces nothing from one.
pub(crate) const UNHOOKED: &str = "this source takes no webhooks";

/// What a webhook delivery asks herald to announce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hooked {
    /// The event it publishes.
    Event(Event),
    /// The event an owner's member publishes, announced only where the
    /// owner's last membership record holds the member when its work runs.
    Member {
        /// The event.
        event: Event,
        /// The forge's id of the member.
        id: String,
        /// Who may see the member, as the delivery names it, where it does.
        visibility: Option<Visibility>,
    },
    /// A tag pushed to a repository the source takes, which a poll of the
    /// source reads in full and announces or holds as it does any tag it
    /// finds (ADR-0032).
    Poll,
    /// Nothing, and why.
    Nothing(&'static str),
}

/// One kind of source: the table it reads, the options it reads into, and
/// how the schema names it.
pub trait SourceKind: 'static {
    /// The kind's name, as `[sources.<name>] kind` writes it.
    const NAME: &str;
    /// What the kind is, as the schema describes a source of it.
    const DESCRIPTION: &str;

    /// The table it reads.
    type File;
    /// The options its table reads into.
    type Options: SourceConfig + Clone;

    /// The options `file` gives, where they are sound.
    ///
    /// # Errors
    ///
    /// The fault that keeps the source out of the config.
    fn checked(file: Self::File) -> Result<Self::Options, Fault>;

    /// `options` with each file beside the config they name read through
    /// `read`.
    ///
    /// # Errors
    ///
    /// The fault that keeps the source out of the config: a file that cannot
    /// be read, or that does not hold what the options take from it.
    fn read(
        options: Self::Options,
        _read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<Self::Options, Fault> {
        Ok(options)
    }
}

/// One source, ready to poll.
pub trait Poll: fmt::Debug + Send + Sync {
    /// The base URL of everything it asks, the only origin its `Http` may
    /// reach (ADR-0007).
    fn api(&self) -> &str;

    /// How many identities its cursor remembers, newest first.
    fn remembered(&self) -> usize;

    /// What it answers through `http` since `cursor`, or nothing where it
    /// says nothing changed.
    fn polled<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        cursor: Option<&'a Cursor>,
    ) -> Boxed<'a, Result<Option<Polled>, Unpolled>>;

    /// How long a tag it finds waits for its Release before it is
    /// announced, where a tag waits (ADR-0032).
    fn tag_window(&self) -> Option<SignedDuration> {
        None
    }
}

/// Why a poll found nothing to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unpolled {
    /// The source could not be reached.
    Unreachable(String),
    /// Its answer could not be read.
    Unreadable(String),
}

impl Unpolled {
    /// The source could not be reached, as `error` says.
    pub fn unreachable(error: impl fmt::Display) -> Self {
        Self::Unreachable(error.to_string())
    }

    /// Its answer could not be read, as `error` says.
    pub fn unreadable(error: impl fmt::Display) -> Self {
        Self::Unreadable(error.to_string())
    }
}

/// A source's options, of any kind.
#[derive(Clone)]
pub struct Source(Arc<dyn SourceConfig>);

impl Source {
    /// `options`, as a source.
    #[must_use]
    pub fn new(options: impl SourceConfig) -> Self {
        Self(Arc::new(options))
    }

    /// The options as their own kind's, where they are a `T`.
    #[must_use]
    pub fn of<T: SourceConfig>(&self) -> Option<&T> {
        self.0.as_any().downcast_ref()
    }
}

impl Deref for Source {
    type Target = dyn SourceConfig;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        self.0.same(other.0.as_any())
    }
}

impl Eq for Source {}

#[cfg(test)]
mod tests;
