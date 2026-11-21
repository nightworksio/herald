use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;

use herald_core::{
    Attempts, DestinationName, Glob, Inbound, Kind, KindSpecs, Profile, Retention, Route, RouteId,
    Sandbox, SecretRef, SourceName, Templates,
};
use herald_kinds::{ConfiguredStore, Destination, Placed, Registering, Source};
use jiff::SignedDuration;

use crate::oidc::{Oidc, Replay};
use crate::peers::Peer;

/// A config, read, resolved and with its templates compiled.
#[derive(Debug, Clone)]
pub struct Config {
    /// The store the ledger and the rest are kept in (ADR-0005).
    pub store: ConfiguredStore,
    /// How long the store keeps what herald records.
    pub retention: Retention,
    /// How many times the store tries a write that lost a race.
    pub attempts: Attempts,
    /// How far a template may go as it renders.
    pub render: Sandbox,
    /// The installation's marker key, where it is set (ADR-0011).
    pub marker_key: Option<SecretRef>,
    /// The destinations, by name.
    pub destinations: BTreeMap<DestinationName, Destination>,
    /// The sources, by name.
    pub sources: BTreeMap<SourceName, Source>,
    /// The routes, in the order the file gives them.
    pub routes: Vec<RouteConfig>,
    /// Every template, compiled: each template's title and body, and each
    /// route's thread name.
    pub templates: Templates,
    /// The HTTP API's apps, by name (ADR-0007).
    pub apps: BTreeMap<String, App>,
    /// The GitHub Apps sources read as, by name (ADR-0041).
    pub github_apps: BTreeMap<String, GithubApp>,
    /// The views of open pull requests, by name, the built-in `queue` among
    /// them (ADR-0055).
    pub views: BTreeMap<String, crate::ViewConfig>,
    /// The reports routes name among their sources, by name, each board's
    /// among them (ADR-0055, ADR-0056).
    pub reports: BTreeMap<String, crate::Report>,
    /// The rules routes name among their sources, by name (ADR-0056).
    pub rules: BTreeMap<String, crate::Rule>,
    /// The models tasks ask, by name (ADR-0057).
    pub models: BTreeMap<String, crate::ModelConfig>,
    /// The closed-set questions views ask where their rules cannot tell,
    /// by name (ADR-0057).
    pub tasks: BTreeMap<String, crate::Task>,
    /// The cadence of polling and housekeeping.
    pub schedule: Schedule,
    /// How a run holds and keeps an announcement.
    pub announce: Announce,
    /// How many routes and sources a run goes through at once.
    pub run: Run,
    /// How doctor judges the config.
    pub doctor: Doctor,
    /// What herald's commands print.
    pub cli: Cli,
    /// How `herald server` listens, and what it takes on at once.
    pub server: Server,
    /// This herald among other instances.
    pub federation: Federation,
    /// The other heralds it trusts, by name.
    pub peers: BTreeMap<String, Peer>,
    /// How herald's own requests reach the services the config names.
    pub http: herald_ports::Outbound,
    /// Every kind of event it knows: those herald ships, and those it
    /// declares.
    pub kinds: KindSpecs,
    /// The keys that name each secret, by the secret's name, such as
    /// `destinations.discord.token`.
    pub secret_keys: BTreeMap<String, Vec<String>>,
}

/// What a run does, and so which of the config's secrets it reads
/// (ADR-0006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Uses {
    /// A run that announces, keeps house or decides follows: each
    /// destination's credential and the marker key.
    Announces,
    /// A run that polls: those, and each source's token.
    Polls,
    /// A run that serves: every secret the config names, those each
    /// destination answers commands with and each webhook is signed with
    /// among them.
    Serves,
}

impl Uses {
    /// Every use, once each.
    pub const ALL: [Self; 3] = [Self::Announces, Self::Polls, Self::Serves];
}

impl Config {
    /// The version of the config this herald reads, as `version` writes it.
    pub const VERSION: i64 = 1;

    /// Whether `route` may take an event of `kind`: a kind not private, to
    /// a route that says whether its place is private, or where `[doctor]
    /// private_kinds` lets an unstated route run. A kind not known is
    /// private.
    #[must_use]
    pub fn places(&self, route: &RouteConfig, kind: &Kind) -> bool {
        route.stated
            || self.doctor.private_kinds != Guard::Refuse
            || self.kinds.get(kind).is_some_and(|spec| !spec.private)
    }

    /// The surface a report's sections leave through to the route `id`,
    /// the one home deciding it from the route's own config: a feed, a push
    /// or a grouped route's digest, which carry no view; else the route,
    /// private only where it says `private = true` and its destination does
    /// not post where anyone may read it, and recallable where it keeps a
    /// living post. A route or destination it does not hold is not private
    /// (ADR-0067).
    #[must_use]
    pub fn surface(&self, id: &RouteId) -> herald_core::Surface {
        let not_private = herald_core::Surface::Route {
            private: false,
            recallable: false,
        };
        let Some(route) = self.routes.iter().find(|route| route.route.id == *id) else {
            return not_private;
        };
        let Some(destination) = self.destinations.get(&route.route.destination) else {
            return not_private;
        };
        if destination.feeding().is_some() {
            return herald_core::Surface::Feed;
        }
        if destination.pushing().is_some() {
            return herald_core::Surface::Push;
        }
        if route.route.delivery.group.is_some() {
            return herald_core::Surface::Digest;
        }
        let public = destination
            .public(&route.options, &route.route.delivery)
            .is_some();
        herald_core::Surface::Route {
            private: route.private && !public,
            recallable: route.living(),
        }
    }

    /// The domains herald's sessions may connect under: each Discord
    /// gateway's and Slack Socket Mode session's own, and those its
    /// destination adds (ADR-0016).
    #[must_use]
    pub fn session_domains(&self) -> BTreeSet<&str> {
        self.destinations
            .values()
            .flat_map(|destination| destination.session_domains())
            .collect()
    }

    /// The name of every secret the config names, once each.
    #[must_use]
    pub fn secrets(&self) -> BTreeSet<&str> {
        self.secrets_beside(self.secrets_used(Uses::Serves))
    }

    /// The name of every secret a run of any use reads, its store's and
    /// each model's among them, once each; a source's write credential is
    /// read by an apply alone, so it is not among them.
    #[must_use]
    pub fn secrets_read(&self) -> BTreeSet<&str> {
        self.secrets_beside(
            Uses::ALL
                .into_iter()
                .flat_map(|uses| self.secrets_used(uses))
                .collect(),
        )
    }

    /// `secrets` with the store's and each model's.
    fn secrets_beside<'a>(&'a self, mut secrets: BTreeSet<&'a str>) -> BTreeSet<&'a str> {
        secrets.extend(self.store.secrets());
        secrets.extend(
            self.models
                .values()
                .filter_map(|model| model.bedrock.as_ref())
                .map(|bedrock| bedrock.session_token.secret.as_str()),
        );
        secrets
    }

    /// The name of every secret a run that `uses` the config reads beside
    /// its store's, which the store reads as it opens, once each.
    #[must_use]
    pub fn secrets_used(&self, uses: Uses) -> BTreeSet<&str> {
        let destinations = self.destinations.values().flat_map(|destination| {
            let mut named: Vec<&SecretRef> = self.credential(destination).into_iter().collect();
            named.extend(destination.reads());
            if uses == Uses::Serves {
                named.extend(destination.secret_refs());
            }
            named
        });
        let sources = self.sources.values().flat_map(|source| match uses {
            Uses::Serves => source.secret_refs(),
            Uses::Polls => source
                .token()
                .into_iter()
                .chain(
                    source
                        .webhook()
                        .filter(|_| source.registering() == Registering::Round),
                )
                .collect(),
            Uses::Announces => Vec::new(),
        });
        let apps = self.github_apps.iter().flat_map(|(name, app)| {
            let read = self.sources.values().any(|source| {
                source
                    .installed()
                    .is_some_and(|installed| installed.app == name)
            });
            let key = (uses >= Uses::Polls && read).then_some(&app.private_key);
            let webhook = app.webhook.as_ref().filter(|_| uses == Uses::Serves);
            key.into_iter().chain(webhook)
        });
        let instance = self
            .federation
            .keys
            .iter()
            .filter(|_| uses == Uses::Serves || self.federation.pages.is_some());
        let pages = self
            .federation
            .pages
            .iter()
            .filter(|pages| uses != Uses::Serves && pages.branch.is_some())
            .map(Pages::token_name);
        let models = self
            .models
            .values()
            .filter(|_| uses >= Uses::Polls)
            .flat_map(crate::ModelConfig::secret_refs);
        destinations
            .chain(models)
            .chain(sources)
            .chain(apps)
            .chain(instance)
            .chain(self.marker_key.as_ref())
            .map(|named| named.secret.as_str())
            .chain(pages)
            .collect()
    }

    /// The secret `destination` is reached with: its own credential, or for
    /// a feed of herald's own or a push to a peer, the instance's newest
    /// key, which proves its envelopes.
    #[must_use]
    pub fn credential<'a>(&'a self, destination: &'a Destination) -> Option<&'a SecretRef> {
        destination.credential().or_else(|| {
            (destination.feeding().is_some() || destination.pushing().is_some())
                .then(|| self.federation.keys.first())
                .flatten()
        })
    }

    /// The path every document of the instance is found under: its static
    /// log's where it writes one, else the server's.
    #[must_use]
    pub fn instance_base(&self) -> &str {
        self.federation
            .pages
            .as_ref()
            .map_or(&self.server.base, |pages| &pages.base)
    }

    /// The keys that name the secret `name`, such as
    /// `destinations.discord.token`; none where only a default names it.
    #[must_use]
    pub fn keys_naming(&self, name: &str) -> &[String] {
        self.secret_keys.get(name).map_or(&[], Vec::as_slice)
    }
}

/// A route, and the options its destination's kind defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteConfig {
    /// The route.
    pub route: Route,
    /// Its destination's own options.
    pub options: Placed,
    /// What its destination can do, as the core plans with it.
    pub profile: Profile,
    /// Whether its place is declared private, so it may take a private or
    /// internal repository's events.
    pub private: bool,
    /// Whether the operator vouches that its place, whose audience herald
    /// cannot check, is private (ADR-0067).
    pub vouched: bool,
    /// Whether it says whether its place is private.
    pub stated: bool,
    /// The private kinds it takes without saying whether its place is
    /// private.
    pub unstated: Vec<Kind>,
}

impl RouteConfig {
    /// Whether it keeps one living post of a report: it asks to, and its
    /// destination edits (ADR-0056).
    #[must_use]
    pub const fn living(&self) -> bool {
        self.route.delivery.living && self.profile.features.edits
    }
}

/// An app of the HTTP API: its credentials, what it may announce to, and
/// the names it pushes into herald sources as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// The SHA-256 of each of its tokens, in lowercase hex.
    pub tokens: Vec<String>,
    /// The ID tokens it takes as credentials (ADR-0064).
    pub oidc: Vec<Oidc>,
    /// The projects it may announce, each a glob.
    pub projects: Vec<Glob>,
    /// The routes it announces to: those it names, or every route that
    /// names it among its sources.
    pub routes: Vec<RouteId>,
    /// The names it stands for when a run holding one of its ID tokens
    /// pushes into a herald source.
    pub peers: BTreeSet<String>,
    /// How often one of its ID tokens may be presented.
    pub replay: Replay,
}

/// A GitHub App herald reads as, through installation tokens it mints
/// (ADR-0041).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubApp {
    /// What its tokens' JWTs are issued by: its client id, or its id.
    pub issuer: String,
    /// Its private key.
    pub private_key: SecretRef,
    /// The REST API its tokens are minted at.
    pub api: String,
    /// The secret its webhook deliveries are signed with, where it takes
    /// them.
    pub webhook: Option<SecretRef>,
    /// Whether its installation tokens are kept in the store between runs.
    pub store_token: bool,
}

impl GithubApp {
    /// The REST API an App's tokens are minted at where `api` is absent.
    pub const API: &str = "https://api.github.com";
}

impl App {
    /// Whether it may announce `project`.
    #[must_use]
    pub fn announces(&self, project: &str) -> bool {
        self.projects.iter().any(|glob| glob.matches(project))
    }

    /// The projects it names one by one, which it announces as: each of its
    /// projects with no wildcard. A glob scopes what it may announce and
    /// claims no project.
    pub fn named(&self) -> impl Iterator<Item = &str> {
        self.projects
            .iter()
            .filter(|glob| glob.specificity().0)
            .map(Glob::as_str)
    }
}

/// How a run holds and keeps an announcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Announce {
    /// How long a run holds an announcement it claimed before another run
    /// may take it over.
    pub lease: SignedDuration,
    /// How often a run writes what a project last announced again when
    /// another write lands between its read and its write.
    pub latest_attempts: usize,
}

impl Announce {
    /// The lease where the config says nothing.
    pub const LEASE: SignedDuration = SignedDuration::from_mins(20);
    /// How often what a project last announced is written where the config
    /// says nothing.
    pub(crate) const LATEST_ATTEMPTS: usize = 3;
}

impl Default for Announce {
    fn default() -> Self {
        Self {
            lease: Self::LEASE,
            latest_attempts: Self::LATEST_ATTEMPTS,
        }
    }
}

/// What doctor does with a config that takes something private to a
/// public place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Guard {
    /// Refuse the config.
    #[default]
    Refuse,
    /// Name it, and let it run.
    Warn,
    /// Say nothing.
    Allow,
}

/// How doctor judges a config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Doctor {
    /// What it does with a route that takes a private or internal
    /// repository's events to a place not declared private.
    pub private_to_public: Guard,
    /// What it does with a route that takes a private kind's events and
    /// says nothing of whether its place is private.
    pub private_kinds: Guard,
}

/// What herald's commands print.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cli {
    /// Whether `herald queue` and `herald rerun` print the rows and plans
    /// of private and internal repositories without `--private`; never
    /// their advisories.
    pub private: bool,
}

/// How many routes and sources a run goes through at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    /// How many routes an event is announced to at once.
    pub routes: NonZeroUsize,
    /// How many sources a poll reads at once.
    pub sources: NonZeroUsize,
}

impl Run {
    /// How many routes are announced to at once where the config says
    /// nothing.
    pub(crate) const ROUTES: NonZeroUsize = NonZeroUsize::new(4).unwrap();
    /// How many sources are read at once where the config says nothing.
    pub(crate) const SOURCES: NonZeroUsize = NonZeroUsize::new(4).unwrap();
}

impl Default for Run {
    fn default() -> Self {
        Self {
            routes: Self::ROUTES,
            sources: Self::SOURCES,
        }
    }
}

/// How often a driver that keeps its own schedule polls and keeps house.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Schedule {
    /// How often sources are polled.
    pub poll: Option<SignedDuration>,
    /// How often threads are kept.
    pub housekeep: Option<SignedDuration>,
}

/// Where the keys inbound activities are checked with are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyCache {
    /// In the server's memory, lost when it stops.
    #[default]
    Memory,
    /// In the store, kept across restarts and shared by every instance.
    Store,
}

/// This herald among other instances: the domain it serves its discovery
/// document on, its Ed25519 keys, newest first, and how long peers keep its
/// key set (ADR-0043, ADR-0044).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Federation {
    /// The domain, where the config gives it.
    pub domain: Option<String>,
    /// The secrets holding its Ed25519 private keys, newest first: the
    /// first signs, and every one verifies.
    pub keys: Vec<SecretRef>,
    /// How long a peer may keep its key set before reading it again.
    pub refresh: SignedDuration,
    /// How long each response of herald's API, and each push it makes to a
    /// peer, stays signed.
    pub signed_for: SignedDuration,
    /// The most bytes a peer's signature field may hold and still be read.
    pub fields: usize,
    /// The peer keys never trusted again.
    pub revoked: Vec<herald_peers::keys::Key>,
    /// How long what each peer asked is kept, by UTC day.
    pub keep_audit: SignedDuration,
    /// How soon before a grant ends doctor names it.
    pub grants_ending: SignedDuration,
    /// How many inbox requests no peer's key signs are kept in the audit.
    pub stranger_rate: herald_core::Rate,
    /// How many replayed nonces of one peer are answered before 429.
    pub replay_rate: herald_core::Rate,
    /// How many requests of one peer refused past its rate have their
    /// nonces kept in each span of `SIGNED_FOR_ALLOWED`.
    pub refused_nonces: u32,
    /// Where its feeds are written as a static signed log, where they are.
    pub pages: Option<Pages>,
}

/// Where herald writes its feeds as a static signed log for a static host
/// to serve (ADR-0060).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pages {
    /// Where the host serves it, with no trailing `/`.
    pub url: String,
    /// The path under `[federation] domain` it is served at; empty at the
    /// root.
    pub base: String,
    /// The branch each writing is committed to, where it is committed to
    /// one.
    pub branch: Option<String>,
    /// The repository holding the branch, where the config names one.
    pub repository: Option<String>,
    /// The secret holding a token that may push to the branch, where the
    /// config names one.
    pub token: Option<SecretRef>,
    /// The API's base URL, where the config names one.
    pub api: Option<String>,
    /// The directory each writing fills with every file, where the config
    /// names one.
    pub artifact: Option<String>,
    /// How long a log no feed changed is left before it is written again.
    pub refresh: SignedDuration,
}

impl Pages {
    /// The branch the log is committed to where `branch` and `artifact`
    /// are absent.
    pub const BRANCH: &str = "herald-pages";
    /// How long an unchanged log is left where `refresh` is absent.
    pub const REFRESH: SignedDuration = SignedDuration::from_hours(24);
    /// The longest `refresh` may be.
    pub const REFRESH_ALLOWED: SignedDuration = SignedDuration::from_hours(7 * 24);

    /// The name of the secret holding the token that pushes to the branch:
    /// the one the config names, else `GITHUB_TOKEN`.
    #[must_use]
    pub fn token_name(&self) -> &str {
        self.token
            .as_ref()
            .map_or(herald_kinds::GITHUB_TOKEN, |token| token.secret.as_str())
    }
}

impl Federation {
    /// How long a peer keeps the key set where `refresh` is absent.
    pub const REFRESH: SignedDuration = SignedDuration::from_hours(24);
    /// The longest `refresh` may be.
    pub const REFRESH_ALLOWED: SignedDuration = SignedDuration::from_hours(7 * 24);
    /// How long a response stays signed where `signed_for` is absent.
    pub const SIGNED_FOR: SignedDuration = SignedDuration::from_mins(5);
    /// The longest `signed_for` may be.
    pub const SIGNED_FOR_ALLOWED: SignedDuration = SignedDuration::from_mins(20);
    /// The most bytes a peer's signature field may hold where `fields` is
    /// absent.
    pub const FIELDS: usize = herald_peers::message::FIELD_MOST;
    /// The most bytes a config may let a signature field hold: four times
    /// the default.
    pub const FIELDS_ALLOWED: usize = Self::FIELDS * 4;
    /// How long a peer's requests are kept where `keep_audit` is absent.
    pub const KEEP_AUDIT: SignedDuration = SignedDuration::from_hours(90 * 24);
    /// The longest `keep_audit` may be.
    pub const KEEP_AUDIT_ALLOWED: SignedDuration = SignedDuration::from_hours(365 * 24);
    /// How soon before a grant ends doctor names it where `grants_ending`
    /// is absent.
    pub const GRANTS_ENDING: SignedDuration = SignedDuration::from_hours(7 * 24);
    /// The longest `grants_ending` may be.
    pub const GRANTS_ENDING_ALLOWED: SignedDuration = SignedDuration::from_hours(90 * 24);
    /// How many refused requests of one peer have their nonces kept where
    /// `refused_nonces` is absent: as many as its default rate lets through
    /// in the longest a request lives.
    pub const REFUSED_NONCES: u32 = 2400;
    /// The most `refused_nonces` may be: as many as the highest rate lets
    /// through in the longest a request lives.
    pub const REFUSED_NONCES_ALLOWED: u32 = 4 * Self::REFUSED_NONCES;
}

impl Default for Federation {
    fn default() -> Self {
        Self {
            domain: None,
            keys: Vec::new(),
            refresh: Self::REFRESH,
            signed_for: Self::SIGNED_FOR,
            fields: Self::FIELDS,
            revoked: Vec::new(),
            keep_audit: Self::KEEP_AUDIT,
            grants_ending: Self::GRANTS_ENDING,
            stranger_rate: crate::Peer::RATE,
            replay_rate: crate::Peer::RATE,
            refused_nonces: Self::REFUSED_NONCES,
            pages: None,
        }
    }
}

/// How `herald server` listens, and what it takes on at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    /// The address it listens on.
    pub listen: String,
    /// The path every endpoint is served under, as `/herald`; empty where
    /// they are served at the root.
    pub base: String,
    /// herald's public address, where the config gives it.
    pub url: Option<String>,
    /// How many pieces of work it does at once.
    pub jobs: usize,
    /// How many pieces of work may wait for a place; a request leaving one
    /// while as many wait is answered 503.
    pub queue: usize,
    /// How many connections it holds at once.
    pub connections: usize,
    /// The most bytes an inbound request's body may hold.
    pub body: usize,
    /// How many actor documents it reads at once.
    pub fetches: NonZeroUsize,
    /// How many autocompletions a session answers at once; one arriving
    /// while as many are answered is dropped.
    pub completions: usize,
    /// How long a session a destination holds hears nothing before it
    /// checks the connection is alive, and then as long again before it
    /// opens another.
    pub silence: SignedDuration,
    /// Where the keys inbound activities are checked with are kept.
    pub keys: KeyCache,
    /// How long a key is used before its document is read again.
    pub keys_refreshed: SignedDuration,
    /// How long after a key is read an activity that does not hold under it
    /// is refused rather than having it read again.
    pub keys_reread: SignedDuration,
    /// The most keys kept in memory.
    pub keys_most: usize,
    /// What a signed inbound request may carry and still be checked.
    pub inbound: Inbound,
    /// How long a request whose work found no place is asked to wait.
    pub busy_retry: SignedDuration,
    /// How long a connection may take to send a request's headers.
    pub read: SignedDuration,
    /// How long a connection is held at most, its one request answered.
    pub held: SignedDuration,
    /// How long the server waits after a connection could not be accepted.
    pub accept_pause: SignedDuration,
    /// How a session a destination holds connects again, and how old a
    /// command one taken up again answers.
    pub sessions: HeldSessions,
    /// How the work a request leaves keeps within its host's limits.
    pub work: Work,
}

/// How the work a request leaves keeps within its host's limits: herald
/// starts no send that could pass them, and the next run sends what is
/// left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Work {
    /// How long the work may run after the answer, where the config bounds
    /// it; the host's own limit where it does not.
    pub after: Option<SignedDuration>,
    /// How much of `after` must be left for a send to start.
    pub spare: SignedDuration,
    /// The most outbound requests a request and its work may make, where
    /// the config bounds it; the host's own limit where it does not.
    pub requests: Option<NonZeroUsize>,
    /// How many of `requests` must be left for a send to start.
    pub spare_requests: usize,
}

impl Work {
    /// How much time must be left for a send to start where the config
    /// says nothing.
    pub(crate) const SPARE: SignedDuration = SignedDuration::from_secs(10);
    /// How many requests must be left for a send to start where the config
    /// says nothing.
    pub(crate) const SPARE_REQUESTS: usize = 4;
}

impl Default for Work {
    fn default() -> Self {
        Self {
            after: None,
            spare: Self::SPARE,
            requests: None,
            spare_requests: Self::SPARE_REQUESTS,
        }
    }
}

/// How a session a destination holds connects again: the first wait,
/// doubled each time it is not taken up, up to the longest; and how old a
/// command a session taken up again answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeldSessions {
    /// The first wait after a session fails to connect.
    pub first_wait: SignedDuration,
    /// The longest wait between attempts.
    pub longest_wait: SignedDuration,
    /// Whether each wait is between half of it and all of it, so sessions
    /// that failed together do not all connect again at once.
    pub jitter: bool,
    /// How long ago a command a session taken up again answers may have
    /// been given; one given earlier is skipped.
    pub stale: SignedDuration,
}

impl HeldSessions {
    /// The first wait where the config says nothing.
    pub(crate) const FIRST_WAIT: SignedDuration = SignedDuration::from_secs(1);
    /// The longest wait where the config says nothing.
    pub(crate) const LONGEST_WAIT: SignedDuration = SignedDuration::from_mins(1);
    /// How old a command may be where the config says nothing.
    pub const STALE: SignedDuration = SignedDuration::from_mins(10);
}

impl Default for HeldSessions {
    fn default() -> Self {
        Self {
            first_wait: Self::FIRST_WAIT,
            longest_wait: Self::LONGEST_WAIT,
            jitter: true,
            stale: Self::STALE,
        }
    }
}

impl Server {
    /// Where `herald server` listens where the config names nowhere: this
    /// machine alone, as a reverse proxy in front of it reaches it.
    pub const LISTEN: &str = "127.0.0.1:8080";
    /// How many pieces of work it does at once where the config says
    /// nothing.
    pub(crate) const JOBS: usize = 8;
    /// How many connections it holds at once where the config says nothing.
    pub(crate) const CONNECTIONS: usize = 256;
    /// The most bytes a body may hold where the config says nothing: 1 MiB.
    pub const BODY: usize = 1 << 20;
    /// The most bytes a config may let a body hold: four times the default.
    pub const BODY_MOST: usize = Self::BODY * 4;
    /// How many actor documents it reads at once where the config says
    /// nothing.
    pub(crate) const FETCHES: NonZeroUsize = NonZeroUsize::new(4).unwrap();
    /// How many autocompletions a session answers at once where the config
    /// says nothing.
    pub(crate) const COMPLETIONS: usize = 4;
    /// How long a key is used before it is read again where the config says
    /// nothing.
    pub(crate) const KEYS_REFRESHED: SignedDuration = SignedDuration::from_hours(1);
    /// How long after a key is read an activity that does not hold under it
    /// is refused, where the config says nothing.
    pub(crate) const KEYS_REREAD: SignedDuration = SignedDuration::from_mins(1);
    /// The most keys kept in memory where the config says nothing.
    pub(crate) const KEYS_MOST: usize = 1024;
    /// How long a session hears nothing before it checks its connection
    /// where the config says nothing.
    pub const SILENCE: SignedDuration = SignedDuration::from_secs(30);
    /// How long a connection may take to send a request's headers, and then
    /// its body.
    pub const READ: SignedDuration = SignedDuration::from_secs(30);
    /// How long a connection is held at most, its one request answered.
    pub const HELD: SignedDuration = SignedDuration::from_secs(90);
    /// How long a request whose work found no place is asked to wait where
    /// the config says nothing.
    pub(crate) const BUSY_RETRY: SignedDuration = SignedDuration::from_secs(30);
    /// How long the server waits after a connection could not be accepted
    /// where the config says nothing.
    pub(crate) const ACCEPT_PAUSE: SignedDuration = SignedDuration::from_millis(100);
}

impl Server {
    /// Whether the keys inbound activities are checked with are kept in the
    /// store, rather than in memory.
    #[must_use]
    pub fn keys_in_store(&self) -> bool {
        self.keys == KeyCache::Store
    }
}

impl Default for Server {
    fn default() -> Self {
        Self {
            listen: Self::LISTEN.to_owned(),
            base: String::new(),
            url: None,
            jobs: Self::JOBS,
            queue: Self::JOBS,
            connections: Self::CONNECTIONS,
            body: Self::BODY,
            fetches: Self::FETCHES,
            completions: Self::COMPLETIONS,
            silence: Self::SILENCE,
            keys: KeyCache::Memory,
            keys_refreshed: Self::KEYS_REFRESHED,
            keys_reread: Self::KEYS_REREAD,
            keys_most: Self::KEYS_MOST,
            inbound: Inbound::DEFAULT,
            busy_retry: Self::BUSY_RETRY,
            read: Self::READ,
            held: Self::HELD,
            accept_pause: Self::ACCEPT_PAUSE,
            sessions: HeldSessions::default(),
            work: Work::default(),
        }
    }
}
