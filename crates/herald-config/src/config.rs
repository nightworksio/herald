use std::collections::{BTreeMap, BTreeSet};

use herald_core::{
    DestinationName, Kind, Profile, Route, RouteId, SecretRef, SourceName, Templates,
};
use herald_sources_forge::Forge;
use herald_sources_registry::Registry;
use jiff::SignedDuration;

/// A config, read, resolved and with its templates compiled.
#[derive(Debug)]
pub struct Config {
    /// The store the ledger and the rest are kept in (ADR-0005).
    pub store: Store,
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
    /// How `herald server` listens, and what it takes on at once.
    pub server: Server,
}

impl Config {
    /// The name of every secret the config names, once each.
    #[must_use]
    pub fn secrets(&self) -> BTreeSet<&str> {
        let destinations = self
            .destinations
            .values()
            .flat_map(|destination| match destination {
                Destination::Discord(options) => {
                    [options.token.as_ref(), options.webhook.as_ref(), None, None]
                }
                Destination::Mastodon(options) => [Some(&options.token), None, None, None],
                Destination::Matrix(options) => {
                    let interactions = options.interactions.as_ref();
                    [
                        Some(&options.token),
                        interactions
                            .and_then(|interactions| interactions.homeserver_token.as_ref()),
                        interactions
                            .and_then(|interactions| interactions.appservice_token.as_ref()),
                        None,
                    ]
                }
                Destination::Slack(options) => {
                    let interactions = options.interactions.as_ref();
                    [
                        options.token.as_ref(),
                        options.webhook.as_ref(),
                        interactions.and_then(|interactions| interactions.signing.as_ref()),
                        interactions.and_then(|interactions| interactions.app.as_ref()),
                    ]
                }
                Destination::ActivityPub(options) => [Some(&options.key), None, None, None],
            });
        let sources = self.sources.values().flat_map(|source| match source {
            Source::Forge(_, options) => [options.token.as_ref(), options.webhook.as_ref()],
            Source::Registry(_, options) => [options.token.as_ref(), None],
            Source::Feed(_) => [None, None],
        });
        let store: Vec<&SecretRef> = match &self.store {
            Store::GitBranch(options) => options.token.iter().collect(),
            Store::RepoVariables(options) => vec![&options.token],
            Store::Postgres(options) => vec![&options.url],
            Store::Dynamodb(options) => options.secrets().collect(),
            _ => Vec::new(),
        };
        destinations
            .chain(sources)
            .flatten()
            .chain(self.marker_key.as_ref())
            .chain(store)
            .map(|named| named.secret.as_str())
            .collect()
    }
}

/// Which store keeps herald's records, and its options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Store {
    /// A SQLite file.
    Sqlite(herald_store_sqlite::StoreOptions),
    /// A Postgres database.
    Postgres(herald_store_postgres::StoreOptions),
    /// Cloudflare D1.
    D1(herald_store_d1::StoreOptions),
    /// A Cloudflare Durable Object's storage.
    DurableObject(herald_store_durable_object::StoreOptions),
    /// An AWS DynamoDB table.
    Dynamodb(herald_store_dynamodb::StoreOptions),
    /// Deno KV.
    DenoKv(herald_store_deno_kv::StoreOptions),
    /// A Fastly KV Store.
    FastlyKv(herald_store_fastly_kv::StoreOptions),
    /// The destinations' own posts.
    Markers,
    /// A branch of the repository.
    GitBranch(herald_store_git_branch::StoreOptions),
    /// The GitHub Actions cache.
    ActionsCache(herald_store_actions_cache::StoreOptions),
    /// The repository's variables.
    RepoVariables(herald_store_repo_variables::StoreOptions),
}

impl Store {
    /// The kind of a SQLite store, as the config spells it.
    pub const SQLITE: &'static str = "sqlite";
    /// The kind of a Postgres store.
    pub const POSTGRES: &'static str = "postgres";
    /// The kind of a Cloudflare D1 store.
    pub const CLOUDFLARE_D1: &'static str = "d1";
    /// The kind of a store in a Cloudflare Durable Object.
    pub const DURABLE_OBJECT: &'static str = "durable-object";
    /// The kind of a DynamoDB store.
    pub const DYNAMODB: &'static str = "dynamodb";
    /// The kind of a Deno KV store.
    pub const DENO_KV: &'static str = "deno-kv";
    /// The kind of a Fastly KV Store store.
    pub const FASTLY_KV: &'static str = "fastly-kv";
    /// The kind of the store kept in the destinations' own posts.
    pub const MARKERS: &'static str = "markers";
    /// The kind of a store on a branch of the repository.
    pub const GIT_BRANCH: &'static str = "git-branch";
    /// The kind of a store in the GitHub Actions cache.
    pub const ACTIONS_CACHE: &'static str = "actions-cache";
    /// The kind of a store in the repository's variables.
    pub const REPO_VARIABLES: &'static str = "repo-variables";
    /// Every kind, as the config spells it.
    pub const KINDS: [&'static str; 11] = [
        Self::SQLITE,
        Self::POSTGRES,
        Self::CLOUDFLARE_D1,
        Self::DURABLE_OBJECT,
        Self::DYNAMODB,
        Self::DENO_KV,
        Self::FASTLY_KV,
        Self::MARKERS,
        Self::GIT_BRANCH,
        Self::ACTIONS_CACHE,
        Self::REPO_VARIABLES,
    ];

    /// The kind's name, as `[store] kind` writes it.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Sqlite(_) => Self::SQLITE,
            Self::Postgres(_) => Self::POSTGRES,
            Self::D1(_) => Self::CLOUDFLARE_D1,
            Self::DurableObject(_) => Self::DURABLE_OBJECT,
            Self::Dynamodb(_) => Self::DYNAMODB,
            Self::DenoKv(_) => Self::DENO_KV,
            Self::FastlyKv(_) => Self::FASTLY_KV,
            Self::Markers => Self::MARKERS,
            Self::GitBranch(_) => Self::GIT_BRANCH,
            Self::ActionsCache(_) => Self::ACTIONS_CACHE,
            Self::RepoVariables(_) => Self::REPO_VARIABLES,
        }
    }
}

/// A destination and its options, by its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    /// Discord.
    Discord(herald_dest_discord::DestinationOptions),
    /// An account on a server speaking the Mastodon client API.
    Mastodon(herald_dest_fediverse::DestinationOptions),
    /// A bot account on a Matrix homeserver.
    Matrix(herald_dest_matrix::DestinationOptions),
    /// Slack, by bot token or webhook.
    Slack(herald_dest_slack::DestinationOptions),
    /// herald's own ActivityPub actors, on its domain.
    ActivityPub(herald_dest_fediverse::ActorOptions),
}

impl Destination {
    /// The kind of a Discord destination, as the config spells it.
    pub const DISCORD: &'static str = "discord";
    /// The kind of a Mastodon destination, as the config spells it.
    pub const MASTODON: &'static str = "mastodon";
    /// The kind of a Matrix destination, as the config spells it.
    pub const MATRIX: &'static str = "matrix";
    /// The kind of a Slack destination, as the config spells it.
    pub const SLACK: &'static str = "slack";
    /// The kind of herald's own ActivityPub actors, as the config spells it.
    pub const ACTIVITYPUB: &'static str = "activitypub";
    /// Every kind, as the config spells it.
    pub const KINDS: [&'static str; 5] = [
        Self::DISCORD,
        Self::MASTODON,
        Self::MATRIX,
        Self::SLACK,
        Self::ACTIVITYPUB,
    ];

    /// The kind's name, as `[destinations.<name>] kind` writes it.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Discord(_) => Self::DISCORD,
            Self::Mastodon(_) => Self::MASTODON,
            Self::Matrix(_) => Self::MATRIX,
            Self::Slack(_) => Self::SLACK,
            Self::ActivityPub(_) => Self::ACTIVITYPUB,
        }
    }

    /// How it answers commands, where it takes any.
    #[must_use]
    pub fn answering(&self) -> Option<herald_core::Answering> {
        match self {
            Self::Discord(options) => options
                .interactions
                .as_ref()
                .map(herald_dest_discord::Interactions::answering),
            Self::Slack(options) => options
                .interactions
                .as_ref()
                .map(herald_dest_slack::Interactions::answering),
            Self::Matrix(options) => options
                .interactions
                .as_ref()
                .map(herald_dest_matrix::Interactions::answering),
            Self::Mastodon(_) | Self::ActivityPub(_) => None,
        }
    }
}

/// A source and its options, by its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A repository's releases on a forge.
    Forge(Forge, herald_sources_forge::SourceOptions),
    /// A package's versions on a registry.
    Registry(Registry, herald_sources_registry::RegistryOptions),
    /// An RSS or Atom feed's entries.
    Feed(herald_sources_feed::FeedOptions),
}

impl Source {
    /// The kind of a feed source, as the config spells it.
    pub const FEED: &'static str = "feed";

    /// The kind's name, as `[sources.<name>] kind` writes it.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Forge(forge, _) => forge.kind(),
            Self::Registry(registry, _) => registry.kind(),
            Self::Feed(_) => Self::FEED,
        }
    }

    /// Every kind, as the config spells it.
    pub const KINDS: [&'static str; 8] = {
        let [github, gitlab, forgejo] = Forge::ALL;
        let [packagist, crates_io, npm, docker_hub] = Registry::ALL;
        [
            github.kind(),
            gitlab.kind(),
            forgejo.kind(),
            packagist.kind(),
            crates_io.kind(),
            npm.kind(),
            docker_hub.kind(),
            Self::FEED,
        ]
    };

    /// The kinds of event the source produces.
    #[must_use]
    pub fn kinds(&self) -> &'static [Kind] {
        match self {
            Self::Forge(..) => &[Kind::Release],
            Self::Registry(..) => &[Kind::PackageVersion],
            Self::Feed(_) => &[Kind::FeedEntry],
        }
    }
}

/// A route, and the options its destination's kind defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteConfig {
    /// The route.
    pub route: Route,
    /// Its destination's own options.
    pub options: RouteOptions,
    /// What its destination can do, as the core plans with it.
    pub profile: Profile,
}

/// A route's options for its destination's kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteOptions {
    /// Discord's.
    Discord(herald_dest_discord::RouteOptions),
    /// Mastodon's.
    Mastodon(herald_dest_fediverse::RouteOptions),
    /// Matrix's.
    Matrix(herald_dest_matrix::RouteOptions),
    /// Slack's.
    Slack(herald_dest_slack::RouteOptions),
    /// herald's own actor's.
    ActivityPub(herald_dest_fediverse::ActorRouteOptions),
}

impl RouteOptions {
    /// Whether every post opens a thread of its own, as a Discord forum's
    /// does, so the route takes `thread` whatever its destination's features
    /// say.
    #[must_use]
    pub fn threads_each_post(&self) -> bool {
        matches!(self, Self::Discord(placed) if placed.forum)
    }

    /// The channel or room the route announces in, where its destination
    /// names one, as a command given there names it.
    #[must_use]
    pub fn place(&self) -> Option<&str> {
        match self {
            Self::Discord(placed) => placed.channel.as_deref(),
            Self::Slack(placed) => placed.channel.as_deref(),
            Self::Matrix(placed) => Some(&placed.room),
            Self::Mastodon(_) | Self::ActivityPub(_) => None,
        }
    }

    /// The opt-in role members subscribe to, which the route mentions with
    /// its announcements: a Discord route's alone.
    #[must_use]
    pub fn opt_in_role(&self) -> Option<String> {
        match self {
            Self::Discord(placed) => placed
                .subscribe
                .as_ref()
                .map(|subscribe| subscribe.role.clone()),
            _ => None,
        }
    }
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
    /// How many announcements it works on at once; a webhook arriving while
    /// as many more wait is answered 503.
    pub jobs: usize,
    /// How many connections it holds at once.
    pub connections: usize,
    /// The most bytes an inbound request's body may hold.
    pub body: usize,
    /// How many actor documents it reads at once.
    pub fetches: usize,
    /// How many of an announcement's inboxes are delivered to at once.
    pub deliveries: usize,
    /// How long a session a destination holds hears nothing before it
    /// checks the connection is alive, and then as long again before it
    /// opens another.
    pub silence: SignedDuration,
    /// Where the keys inbound activities are checked with are kept.
    pub keys: KeyCache,
    /// How long a key is used before its document is read again.
    pub keys_refreshed: SignedDuration,
    /// The most keys kept in memory.
    pub keys_most: usize,
}

impl Server {
    /// Where `herald server` listens where the config names nowhere: this
    /// machine alone, as a reverse proxy in front of it reaches it.
    pub const LISTEN: &'static str = "127.0.0.1:8080";
    /// How many announcements it works on at once where the config says
    /// nothing.
    pub const JOBS: usize = 8;
    /// How many connections it holds at once where the config says nothing.
    pub const CONNECTIONS: usize = 256;
    /// The most bytes a body may hold where the config says nothing: 1 MiB.
    pub const BODY: usize = 1 << 20;
    /// How many actor documents it reads at once where the config says
    /// nothing.
    pub const FETCHES: usize = 4;
    /// How many inboxes it delivers to at once where the config says
    /// nothing.
    pub const DELIVERIES: usize = 8;
    /// How long a key is used before it is read again where the config says
    /// nothing.
    pub const KEYS_REFRESHED: SignedDuration = SignedDuration::from_hours(1);
    /// The most keys kept in memory where the config says nothing.
    pub const KEYS_MOST: usize = 1024;
    /// How long a session hears nothing before it checks its connection
    /// where the config says nothing.
    pub const SILENCE: SignedDuration = SignedDuration::from_secs(30);
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
            jobs: Self::JOBS,
            connections: Self::CONNECTIONS,
            body: Self::BODY,
            fetches: Self::FETCHES,
            deliveries: Self::DELIVERIES,
            silence: Self::SILENCE,
            keys: KeyCache::Memory,
            keys_refreshed: Self::KEYS_REFRESHED,
            keys_most: Self::KEYS_MOST,
        }
    }
}
