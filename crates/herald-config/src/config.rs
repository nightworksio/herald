use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;

use herald_core::{
    Attempts, DestinationName, Inbound, KindSpecs, Profile, Retention, Route, RouteId, Sandbox,
    SecretRef, SourceName, Templates,
};
use herald_kinds::{ConfiguredStore, Destination, Placed, Source};
use jiff::SignedDuration;

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
    /// The cadence of polling and housekeeping.
    pub schedule: Schedule,
    /// How a run holds and keeps an announcement.
    pub announce: Announce,
    /// How many routes and sources a run goes through at once.
    pub run: Run,
    /// How `herald server` listens, and what it takes on at once.
    pub server: Server,
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

impl Config {
    /// The version of the config this herald reads, as `version` writes it.
    pub const VERSION: i64 = 1;

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
        let mut secrets = self.secrets_used(Uses::Serves);
        secrets.extend(self.store.secrets());
        secrets
    }

    /// The name of every secret a run that `uses` the config reads beside
    /// its store's, which the store reads as it opens, once each.
    #[must_use]
    pub fn secrets_used(&self, uses: Uses) -> BTreeSet<&str> {
        let destinations = self.destinations.values().flat_map(|destination| {
            if uses == Uses::Serves {
                destination.secret_refs()
            } else {
                destination.credential().into_iter().collect()
            }
        });
        let sources = self.sources.values().flat_map(|source| match uses {
            Uses::Serves => source.secret_refs(),
            Uses::Polls => source.token().into_iter().collect(),
            Uses::Announces => Vec::new(),
        });
        destinations
            .chain(sources)
            .chain(self.marker_key.as_ref())
            .map(|named| named.secret.as_str())
            .collect()
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
}

/// An app of the HTTP API: its token's hash and what it may announce to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// The SHA-256 of each of its tokens, in lowercase hex.
    pub tokens: Vec<String>,
    /// The projects it may announce.
    pub projects: Vec<String>,
    /// The routes it announces to: those it names, or every route that
    /// names it among its sources.
    pub routes: Vec<RouteId>,
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

/// How `herald server` listens, and what it takes on at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    /// The address it listens on.
    pub listen: String,
    /// The path every endpoint is served under, as `/herald`; empty where
    /// they are served at the root.
    pub base: String,
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
        }
    }
}
