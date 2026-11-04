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
}

impl Config {
    /// The name of every secret the config names, once each.
    #[must_use]
    pub fn secrets(&self) -> BTreeSet<&str> {
        let destinations = self
            .destinations
            .values()
            .flat_map(|destination| match destination {
                Destination::Discord(options) => [options.token.as_ref(), options.webhook.as_ref()],
                Destination::Mastodon(options) => [Some(&options.token), None],
                Destination::Matrix(options) => [Some(&options.token), None],
                Destination::Slack(options) => [options.token.as_ref(), options.webhook.as_ref()],
                Destination::ActivityPub(options) => [Some(&options.key), None],
            });
        let sources = self.sources.values().map(|source| match source {
            Source::Forge(_, options) => options.token.as_ref(),
            Source::Registry(_, options) => options.token.as_ref(),
            Source::Feed(_) => None,
        });
        let store = match &self.store {
            Store::GitBranch(options) => options.token.as_ref(),
            Store::RepoVariables(options) => Some(&options.token),
            Store::Postgres(options) => Some(&options.url),
            _ => None,
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
    D1,
    /// An AWS DynamoDB table.
    Dynamodb,
    /// Deno KV.
    DenoKv,
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
    /// The kind of a DynamoDB store.
    pub const DYNAMODB: &'static str = "dynamodb";
    /// The kind of a Deno KV store.
    pub const DENO_KV: &'static str = "deno-kv";
    /// The kind of the store kept in the destinations' own posts.
    pub const MARKERS: &'static str = "markers";
    /// The kind of a store on a branch of the repository.
    pub const GIT_BRANCH: &'static str = "git-branch";
    /// The kind of a store in the GitHub Actions cache.
    pub const ACTIONS_CACHE: &'static str = "actions-cache";
    /// The kind of a store in the repository's variables.
    pub const REPO_VARIABLES: &'static str = "repo-variables";
    /// Every kind, as the config spells it.
    pub const KINDS: [&'static str; 9] = [
        Self::SQLITE,
        Self::POSTGRES,
        Self::CLOUDFLARE_D1,
        Self::DYNAMODB,
        Self::DENO_KV,
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
            Self::D1 => Self::CLOUDFLARE_D1,
            Self::Dynamodb => Self::DYNAMODB,
            Self::DenoKv => Self::DENO_KV,
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
}

/// An app of the HTTP API: its token's hash and what it may announce to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// The SHA-256 of its token, in lowercase hex.
    pub token_sha256: String,
    /// The projects it may announce.
    pub projects: Vec<String>,
    /// The routes it may announce to.
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
