use std::collections::BTreeMap;

use schemars::JsonSchema;

use crate::file::{AppFile, MarkersFile, RouteFile, ScheduleFile, TemplateFile};

/// `[store]`: one store, by its kind.
#[derive(JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
#[schemars(rename = "Store")]
#[allow(dead_code, reason = "read only by the schema's derive")]
enum StoreSchema {
    /// A SQLite file, on a volume or a persistent disk.
    Sqlite {},
    /// A Postgres database.
    Postgres {},
    /// Cloudflare D1, through the Worker's binding.
    D1 {},
    /// An AWS DynamoDB table.
    Dynamodb {},
    /// Deno KV, on Deno Deploy.
    DenoKv {},
    /// The destinations' own posts.
    Markers {},
    /// One file per record on a `herald-state` branch.
    GitBranch(herald_store_git_branch::StoreOptions),
    /// Generations of the GitHub Actions cache.
    ActionsCache(herald_store_actions_cache::StoreOptions),
    /// The repository's variables.
    RepoVariables(herald_store_repo_variables::StoreOptions),
}

/// A destination, by its kind.
#[derive(JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[schemars(rename = "Destination")]
#[allow(dead_code, reason = "read only by the schema's derive")]
enum DestinationSchema {
    /// Discord, by bot token or webhook.
    Discord(herald_dest_discord::DestinationOptions),
    /// An account on a server speaking the Mastodon client API.
    Mastodon(herald_dest_fediverse::DestinationOptions),
    /// A bot account on a Matrix homeserver.
    Matrix(herald_dest_matrix::DestinationOptions),
    /// Slack, by bot token or webhook.
    Slack(herald_dest_slack::DestinationOptions),
    /// herald's own ActivityPub actors, on its domain.
    #[serde(rename = "activitypub")]
    ActivityPub(herald_dest_fediverse::ActorOptions),
}

/// A source, by its kind.
#[derive(JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[schemars(rename = "Source")]
#[allow(dead_code, reason = "read only by the schema's derive")]
enum SourceSchema {
    /// A GitHub repository's releases.
    #[serde(rename = "github")]
    GitHub(herald_sources_forge::SourceOptions),
    /// A GitLab project's releases.
    #[serde(rename = "gitlab")]
    GitLab(herald_sources_forge::SourceOptions),
    /// A Forgejo or Gitea repository's releases.
    Forgejo(herald_sources_forge::SourceOptions),
    /// A package's versions on Packagist.
    Packagist(herald_sources_registry::RegistryOptions),
    /// A crate's versions on crates.io.
    #[serde(rename = "crates-io")]
    CratesIo(herald_sources_registry::RegistryOptions),
    /// A package's versions on the npm registry.
    Npm(herald_sources_registry::RegistryOptions),
    /// A repository's tags on Docker Hub.
    DockerHub(herald_sources_registry::RegistryOptions),
    /// An RSS or Atom feed's entries.
    Feed(herald_sources_feed::FeedOptions),
}

/// A route: what it takes, where it announces, and its destination's own
/// options, as its destination's kind defines them.
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(rename = "Route")]
#[allow(dead_code, reason = "read only by the schema's derive")]
enum RouteSchema {
    /// A route to a Discord destination.
    Discord(DiscordRoute),
    /// A route to a Mastodon destination.
    Mastodon(MastodonRoute),
    /// A route to a Matrix destination.
    Matrix(MatrixRoute),
    /// A route to a Slack destination.
    Slack(SlackRoute),
    /// A route announcing as one of herald's own ActivityPub actors.
    ActivityPub(ActivityPubRoute),
}

/// A route to a Discord destination.
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
#[allow(dead_code, reason = "read only by the schema's derive")]
struct DiscordRoute {
    #[serde(flatten)]
    common: RouteFile,
    #[serde(flatten)]
    discord: herald_dest_discord::RouteOptions,
}

/// A route to a Matrix destination.
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
#[allow(dead_code, reason = "read only by the schema's derive")]
struct MatrixRoute {
    #[serde(flatten)]
    common: RouteFile,
    #[serde(flatten)]
    matrix: herald_dest_matrix::RouteOptions,
}

/// A route announcing as one of herald's own ActivityPub actors.
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
#[allow(dead_code, reason = "read only by the schema's derive")]
struct ActivityPubRoute {
    #[serde(flatten)]
    common: RouteFile,
    #[serde(flatten)]
    actor: herald_dest_fediverse::ActorRouteOptions,
}

/// A route to a Slack destination.
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
#[allow(dead_code, reason = "read only by the schema's derive")]
struct SlackRoute {
    #[serde(flatten)]
    common: RouteFile,
    #[serde(flatten)]
    slack: herald_dest_slack::RouteOptions,
}

/// A route to a Mastodon destination.
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
#[allow(dead_code, reason = "read only by the schema's derive")]
struct MastodonRoute {
    #[serde(flatten)]
    common: RouteFile,
    #[serde(flatten)]
    mastodon: herald_dest_fediverse::RouteOptions,
}

/// `herald.toml`: herald's whole config (ADR-0006).
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "herald.toml")]
#[allow(dead_code, reason = "read only by the schema's derive")]
struct FileSchema {
    /// The version of the config: 1.
    version: u8,
    /// The store herald keeps its records in.
    store: StoreSchema,
    /// The installation's marker key.
    markers: Option<MarkersFile>,
    /// The places herald posts to, by name.
    #[serde(default)]
    destinations: BTreeMap<String, DestinationSchema>,
    /// What herald listens to, by name.
    #[serde(default)]
    sources: BTreeMap<String, SourceSchema>,
    /// Which events go where, rendered how.
    #[serde(default)]
    routes: Vec<RouteSchema>,
    /// The templates routes render with, by name.
    #[serde(default)]
    templates: BTreeMap<String, TemplateFile>,
    /// The HTTP API's apps, by name.
    #[serde(default)]
    apps: BTreeMap<String, AppFile>,
    /// How often a driver that keeps its own schedule polls and keeps house.
    schedule: Option<ScheduleFile>,
}

/// The JSON Schema of `herald.toml`, as committed in `herald.schema.json`.
#[must_use]
pub fn schema() -> String {
    let schema = schemars::schema_for!(FileSchema);
    format!(
        "{}\n",
        serde_json::to_string_pretty(&schema).unwrap_or_default()
    )
}
