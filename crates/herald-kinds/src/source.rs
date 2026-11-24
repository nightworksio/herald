use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::num::NonZeroUsize;
use std::ops::Deref;
use std::sync::Arc;

use herald_core::{
    Cursor, Event, EventsRead, Folding, Grant, Kind, Listed, Listing, Origin, Owner, Polled, Reach,
    RouteId, Scheme, SecretRef, Visibility, Zoning,
};
use herald_ports::{Base, Boxed, DynHttp};
use jiff::{SignedDuration, Timestamp};

pub use herald_core::Installation;

use crate::{Credential, Fault, Itself, Most, Peering, Pull, Queue, QueueCost, Receive};

/// A GitHub App a source reads as: the App's name, the REST API the source
/// reads, and the installation its tokens are minted for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed<'a> {
    /// The App's name, as `[github_apps.<name>]` writes it.
    pub app: &'a str,
    /// The REST API the source reads.
    pub api: String,
    /// The installation its tokens are minted for.
    pub installation: Installation<'a>,
}

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
    fn kinds(&self) -> Vec<Kind>;

    /// How many of its newest events the first poll announces (ADR-0003).
    fn backfill(&self) -> usize;

    /// Who signs its webhooks, and how, where it takes them (ADR-0007).
    fn scheme(&self) -> Option<Scheme> {
        None
    }

    /// What the webhook delivery `payload` with `headers` says of the pull
    /// requests of the repository it reads, for kept mode (ADR-0061):
    /// nothing where it is of none.
    ///
    /// # Errors
    ///
    /// Why the delivery could not be read.
    fn folded(
        &self,
        _headers: &[(&str, &str)],
        _payload: &[u8],
    ) -> Result<Option<Folding>, String> {
        Ok(None)
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

    /// Whether its events may be of any kind the config declares, rather
    /// than only [`SourceConfig::kinds`] (ADR-0049).
    fn any_kind(&self) -> bool {
        false
    }

    /// Whose feed it pulls, where it pulls another herald's (ADR-0049).
    fn peering(&self) -> Option<Peering<'_>> {
        None
    }

    /// What each peer may do with it, by the peer's name, where it takes
    /// grants at all.
    fn grants(&self) -> Option<&BTreeMap<String, Grant>> {
        None
    }

    /// The source pulling from its peer, served at `url`, where it pulls
    /// from a peer of `[peers]`.
    fn joined(&self, _url: &str) -> Option<Source> {
        None
    }

    /// Whether the owner's deliveries keep its membership between listings.
    fn pushed(&self) -> bool {
        false
    }

    /// The GitHub App it reads as, where it reads as one (ADR-0041).
    fn installed(&self) -> Option<Installed<'_>> {
        None
    }

    /// Who registers a webhook on each of its owner's members.
    fn registering(&self) -> Registering {
        Registering::Never
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

    /// The repository it announces for, where it names one.
    fn standing(&self) -> Option<Standing> {
        None
    }

    /// How many identities any one instance may have its cursor hold,
    /// where it takes another herald's envelopes (ADR-0049).
    fn share(&self) -> Option<usize> {
        None
    }

    /// How long after a static signed log it reads was written it is still
    /// taken, where it reads one (ADR-0060).
    fn stale_after(&self) -> Option<jiff::SignedDuration> {
        None
    }

    /// How much its cursor holds where it remembers every identity it
    /// lists rather than the newest few, where `[peers]` names `peers`
    /// instances (ADR-0042).
    fn remembers(&self, _peers: usize) -> Option<Remembers> {
        None
    }

    /// What takes the envelopes its peers push to it, where any may
    /// (ADR-0054).
    fn receiver(&self) -> Option<Arc<dyn Receive>> {
        None
    }

    /// It, its cursor fitted to records of at most `most` bytes where
    /// `[peers]` names `peers` instances, where its options name no bound;
    /// nothing where it cannot (ADR-0042).
    fn fitted(&self, _most: usize, _peers: usize) -> Option<Source> {
        None
    }

    /// What reads its repository's open pull requests with `token` where it
    /// has one, where it names one repository on a forge.
    fn queue(&self, _token: Option<Credential>) -> Option<Arc<dyn Queue>> {
        None
    }

    /// The credential it re-runs planned runs with, where it names one
    /// (ADR-0068). It is never among [`SourceConfig::secret_refs`]: only
    /// an operator's apply reads it.
    fn write(&self) -> Option<crate::WriteCredential<'_>> {
        None
    }

    /// What a merge queue asking `asked` of it is given, or why it takes
    /// none so; none where it is no forge (ADR-0087).
    fn merging(
        &self,
        _asked: &crate::MergeAsked<'_>,
    ) -> Option<Result<crate::Merging, crate::Unmerged>> {
        None
    }

    /// How an operator signs in at its forge to apply a re-run, or why none
    /// can (ADR-0068).
    ///
    /// # Errors
    ///
    /// Where it names no sign-in, or its platform has none.
    fn signing(&self) -> Result<crate::SignIn, &'static str> {
        Err(crate::UNSIGNED)
    }

    /// What re-runs planned jobs of its repositories, reading with `read`
    /// where it has one and calling with the write credential alone; or
    /// why nothing can, in plain words (ADR-0068).
    ///
    /// # Errors
    ///
    /// Where it re-runs nothing.
    fn rerunner(
        &self,
        _read: Option<Credential>,
    ) -> Result<Arc<dyn crate::Rerunner>, &'static str> {
        Err(crate::UNWRITTEN)
    }

    /// What reading at most `most` of its open pull requests costs each
    /// round, where [`SourceConfig::queue`] reads them.
    fn queue_cost(&self, _most: Most) -> Option<QueueCost> {
        None
    }

    /// How it names the areas of the repository at the full path
    /// `repository`: the narrowest override's way that sets one, or its
    /// own; none where it names none (ADR-0071, ADR-0073).
    fn areas(&self, _repository: &str) -> Option<&Zoning> {
        None
    }

    /// How it reads the CODEOWNERS file of the repository at the full path
    /// `repository`, where its areas are read from one (ADR-0073).
    fn codeowning(&self, _repository: &str) -> Option<crate::Codeowning> {
        None
    }

    /// Whether it or one of its overrides reads areas from CODEOWNERS, so
    /// their names are known only as each file is read (ADR-0073).
    fn codeowned(&self) -> bool {
        false
    }

    /// What doctor says of how it reads CODEOWNERS (ADR-0073).
    fn codeowners_notes(&self) -> Vec<String> {
        Vec::new()
    }

    /// The name of each area it or one of its overrides names (ADR-0071).
    fn area_names(&self) -> BTreeSet<String> {
        BTreeSet::new()
    }

    /// Whether a view showing its load reads its forge's own teams beside
    /// the config's (ADR-0077).
    fn forge_teams(&self) -> bool {
        false
    }

    /// What doctor says of how it reads its forge's own teams, where it
    /// reads them, after its name (ADR-0077).
    fn teams_note(&self) -> Option<String> {
        None
    }

    /// What polls it, reading with `token` where it has one, and watching
    /// what it announced for an edit where `edits` says a route edits its
    /// posts (ADR-0034).
    fn poller(&self, token: Option<Credential>, edits: bool) -> Polling;
}

/// The repository a source or an owner's member announces for, so a
/// container image and its repository's own source may share a project
/// (ADR-0040).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    /// The repository, as its API base and full path, ignoring case.
    pub repository: String,
    /// Whether it is a container image linked to the repository, rather
    /// than the repository itself.
    pub image: bool,
}

impl Standing {
    /// The repository at `path` on the API at `api`, or an image linked to
    /// it where `image` says so.
    #[must_use]
    pub fn new(api: &str, path: &str, image: bool) -> Self {
        Self {
            repository: format!("{}/{path}", api.trim_end_matches('/')).to_lowercase(),
            image,
        }
    }

    /// Whether it may share a project with `other`: an image and the
    /// repository it is linked to.
    #[must_use]
    pub fn shares(&self, other: &Self) -> bool {
        self.repository == other.repository && self.image != other.image
    }
}

/// How much a cursor holds that remembers every identity its source lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Remembers {
    /// The most identities it remembers.
    pub identities: usize,
    /// The most bytes one identity takes in a record.
    pub each: usize,
    /// The key of the source's table that bounds `identities`.
    pub key: &'static str,
    /// What chose `identities`.
    pub bound: Bound,
    /// How many identities each one `key` names stands for.
    pub per: usize,
    /// The most bytes it holds beside its identities whatever `key` names.
    pub beside: usize,
    /// The other keys of the source's table that bound `beside`.
    pub besides: &'static [&'static str],
}

/// What chose how many identities a cursor remembering every one holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    /// The source's options, which a store keeping smaller records refuses.
    Named,
    /// The kind's default, which a store keeping smaller records lowers.
    Default,
    /// The store, as the most its records hold.
    Fitted,
}

impl Remembers {
    /// The most bytes a cursor remembering `identities` takes in a record,
    /// what else it holds counted as [`Remembers::REST`] and `beside`.
    #[must_use]
    pub const fn bytes(&self) -> usize {
        self.identities
            .saturating_mul(self.each)
            .saturating_add(self.beside)
            .saturating_add(Self::REST)
    }

    /// Whether a cursor remembering its identities fits in `most` bytes.
    #[must_use]
    pub const fn fits(&self, most: usize) -> bool {
        self.bytes() <= most
    }

    /// The most identities whose cursor fits in `most` bytes.
    #[must_use]
    pub const fn fitting(&self, most: usize) -> usize {
        match most
            .saturating_sub(Self::REST)
            .saturating_sub(self.beside)
            .checked_div(self.each)
        {
            Some(fitting) => fitting,
            None => usize::MAX,
        }
    }

    /// The most `key` may name for a cursor to fit in `most` bytes.
    #[must_use]
    pub const fn named(&self, most: usize) -> usize {
        match self.fitting(most).checked_div(self.per) {
            Some(named) => named,
            None => usize::MAX,
        }
    }

    /// The most bytes a cursor holds beside its identities: its validators,
    /// project and marks, and the record's own key and seal.
    pub const REST: usize = 4096;
}

/// What a source's own check of what it reaches found wrong: the key of its
/// table to change, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probed {
    /// The key.
    pub key: &'static str,
    /// Why.
    pub why: String,
}

/// How a source is polled: as one, as each member an owner lists, or as
/// another herald's feed.
#[derive(Debug, Clone)]
pub enum Polling {
    /// One poll.
    One(Arc<dyn Poll>),
    /// An owner's members, each polled on its own.
    Many(Arc<dyn Expand>),
    /// Another herald's feed, pulled through the keys trusted for it.
    Pull(Arc<dyn Pull>),
    /// Nothing: the source takes only what its peers push to it.
    Unpolled,
}

impl Polling {
    /// The base URL of everything it asks, the only origin its `Http` may
    /// reach (ADR-0007).
    #[must_use]
    pub fn api(&self) -> &str {
        match self {
            Self::One(poll) => poll.api(),
            Self::Many(expand) => expand.api(),
            Self::Pull(pull) => pull.api(),
            Self::Unpolled => "",
        }
    }

    /// What its `Http` may reach: the origin of [`Polling::api`], and any
    /// other base it asks (ADR-0007).
    #[must_use]
    pub fn base(&self) -> Base<'_> {
        let bases = match self {
            Self::One(poll) => poll.bases(),
            Self::Many(expand) => expand.bases(),
            Self::Pull(_) => &[],
            Self::Unpolled => return Base::Among(&[]),
        };
        if bases.is_empty() {
            Base::At(self.api())
        } else {
            Base::Among(bases)
        }
    }

    /// What its own check of what it reaches through `http` finds wrong,
    /// for the doctor to name.
    pub fn probed<'a>(&'a self, http: &'a dyn DynHttp) -> Boxed<'a, Vec<Probed>> {
        match self {
            Self::One(poll) => poll.probed(http),
            Self::Many(expand) => expand.probed(http),
            Self::Pull(_) | Self::Unpolled => Box::pin(std::future::ready(Vec::new())),
        }
    }
}

/// A source that expands to an owner's repositories, each polled on its
/// own.
pub trait Expand: fmt::Debug + Send + Sync {
    /// The base URL of everything it asks, the only origin its `Http` may
    /// reach (ADR-0007) unless [`Expand::bases`] names others.
    fn api(&self) -> &str;

    /// Every base URL it asks, `api` among them, where it asks more than
    /// `api`'s origin; none where it asks only that.
    fn bases(&self) -> &[String] {
        &[]
    }

    /// The key of its table that names the owner it lists.
    fn owner_key(&self) -> &'static str {
        "owner"
    }

    /// What one of its members is called, and what several are.
    fn member_nouns(&self) -> (&'static str, &'static str) {
        ("repository", "repositories")
    }

    /// The repository the member `member` announces for, where it is one or
    /// is linked to one.
    fn standing(&self, _member: &Listed) -> Option<Standing> {
        None
    }

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
    /// `http` in the round at `now`, in order, with what the round read
    /// once as `rounded`: polled together where the source batches them,
    /// and each through its own poll otherwise.
    fn polled<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        members: &'a [(&'a Listed, Option<&'a Cursor>)],
        _rounded: &'a Rounded,
        now: Timestamp,
    ) -> Boxed<'a, Vec<Result<Option<Polled>, Unpolled>>> {
        Box::pin(each_polled(self, http, members, now))
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

    /// Whether the owner's deliveries keep its membership between listings,
    /// so it lists only where it holds no listing, its selection changed,
    /// or a delivery said the owner's repositories changed.
    fn pushed(&self) -> bool;

    /// Who registers a webhook on each member.
    fn registering(&self) -> Registering {
        Registering::Never
    }

    /// A webhook on the member `repository`, pointing at `url` and signed
    /// with `secret`, registered through `http` where it has none that
    /// points at `url`.
    fn register<'a>(
        &'a self,
        _http: &'a dyn DynHttp,
        _repository: &'a Listed,
        (_url, _secret): (&'a str, &'a herald_ports::Secret),
    ) -> Boxed<'a, Result<Registered, Unpolled>> {
        Box::pin(std::future::ready(Ok(Registered::Found)))
    }

    /// Whether the member `repository` has a webhook pointing at `url`,
    /// asked through `http`.
    fn hooked<'a>(
        &'a self,
        _http: &'a dyn DynHttp,
        _repository: &'a Listed,
        _url: &'a str,
    ) -> Boxed<'a, Result<bool, Unpolled>> {
        Box::pin(std::future::ready(Ok(true)))
    }

    /// Why its token may not register a webhook on a member, asked through
    /// `http`, where it may not.
    fn may_register<'a>(&'a self, _http: &'a dyn DynHttp) -> Boxed<'a, Result<(), String>> {
        Box::pin(std::future::ready(Ok(())))
    }

    /// What its own check of what it reaches through `http` finds wrong,
    /// beyond its listing; nothing where it checks nothing more.
    fn probed<'a>(&'a self, _http: &'a dyn DynHttp) -> Boxed<'a, Vec<Probed>> {
        Box::pin(std::future::ready(Vec::new()))
    }
}

/// Who registers a webhook on each member of an owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registering {
    /// No one.
    Never,
    /// The operator, through `herald hooks`.
    Setup,
    /// The operator, and each poll round for a member that has none.
    Round,
}

/// What registering a member's webhook did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registered {
    /// It had one already.
    Found,
    /// It has one now.
    Made,
}

/// What each of `members`, each with its cursor, answers through `http`
/// in the round at `now`, each through the poll `expand` gives it, in
/// order.
pub async fn each_polled<E: Expand + ?Sized>(
    expand: &E,
    http: &dyn DynHttp,
    members: &[(&Listed, Option<&Cursor>)],
    now: Timestamp,
) -> Vec<Result<Option<Polled>, Unpolled>> {
    let mut answered = Vec::new();
    for (repository, cursor) in members {
        answered.push(expand.member(repository).polled(http, *cursor, now).await);
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
    /// The owner's repositories changed, so the owner is listed again and
    /// its members polled.
    Relist,
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
    /// reach (ADR-0007) unless [`Poll::bases`] names others.
    fn api(&self) -> &str;

    /// Every base URL it asks, `api` among them, where it asks more than
    /// `api`'s origin; none where it asks only that.
    fn bases(&self) -> &[String] {
        &[]
    }

    /// How many identities its cursor remembers, newest first.
    fn remembered(&self) -> usize;

    /// What it answers through `http` since `cursor` in the round at
    /// `now`, which bounds any time its answers claim, or nothing where it
    /// says nothing changed.
    fn polled<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        cursor: Option<&'a Cursor>,
        now: Timestamp,
    ) -> Boxed<'a, Result<Option<Polled>, Unpolled>>;

    /// How long a tag it finds waits for its Release before it is
    /// announced, where a tag waits (ADR-0032).
    fn tag_window(&self) -> Option<SignedDuration> {
        None
    }

    /// What its own check of what it reaches through `http` finds wrong;
    /// nothing where it checks nothing.
    fn probed<'a>(&'a self, _http: &'a dyn DynHttp) -> Boxed<'a, Vec<Probed>> {
        Box::pin(std::future::ready(Vec::new()))
    }
}

/// Why a poll found nothing to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unpolled {
    /// The source could not be reached.
    Unreachable(String),
    /// Its answer could not be read.
    Unreadable(String),
    /// Its forge answered that the rate limit on its token or address was
    /// reached, as the reason says, with when it resets where the forge
    /// said: never a refusal, so nothing folds in its place.
    RateLimited(String),
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

    /// Why the source was not read, in plain words.
    #[must_use]
    pub fn into_reason(self) -> String {
        match self {
            Self::Unreachable(said) | Self::Unreadable(said) | Self::RateLimited(said) => said,
        }
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
