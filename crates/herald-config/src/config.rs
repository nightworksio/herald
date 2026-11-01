use std::collections::BTreeMap;

use herald_core::{DestinationName, Kind, Route, RouteId, SecretRef, SourceName, Templates};
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

/// Which store keeps herald's records, and its options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Store {
    /// A SQLite file.
    Sqlite,
    /// A Postgres database.
    Postgres,
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
    /// The kind's name, as `[store] kind` writes it.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
            Self::D1 => "d1",
            Self::Dynamodb => "dynamodb",
            Self::DenoKv => "deno-kv",
            Self::Markers => "markers",
            Self::GitBranch(_) => "git-branch",
            Self::ActionsCache(_) => "actions-cache",
            Self::RepoVariables(_) => "repo-variables",
        }
    }

    /// Every kind, as the config spells it.
    pub const KINDS: [&'static str; 9] = [
        "sqlite",
        "postgres",
        "d1",
        "dynamodb",
        "deno-kv",
        "markers",
        "git-branch",
        "actions-cache",
        "repo-variables",
    ];
}

/// A destination and its options, by its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    /// Discord.
    Discord(herald_dest_discord::DestinationOptions),
}

/// A source and its options, by its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A GitHub repository's releases.
    GitHub(herald_sources_forge::SourceOptions),
}

impl Source {
    /// The kinds of event the source produces.
    #[must_use]
    pub fn kinds(&self) -> &'static [Kind] {
        match self {
            Self::GitHub(_) => &[Kind::Release],
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
}

/// A route's options for its destination's kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteOptions {
    /// Discord's.
    Discord(herald_dest_discord::RouteOptions),
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
