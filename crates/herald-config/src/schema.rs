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
    ActionsCache {},
    /// The repository's variables.
    RepoVariables {},
}

/// A destination, by its kind.
#[derive(JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[schemars(rename = "Destination")]
#[allow(dead_code, reason = "read only by the schema's derive")]
enum DestinationSchema {
    /// Discord, by bot token or webhook.
    Discord(herald_dest_discord::DestinationOptions),
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
}

/// A route: what it takes, where it announces, and its destination's own
/// options.
#[derive(JsonSchema)]
#[schemars(rename = "Route", deny_unknown_fields)]
#[allow(dead_code, reason = "read only by the schema's derive")]
struct RouteSchema {
    #[serde(flatten)]
    common: RouteFile,
    #[serde(flatten)]
    discord: herald_dest_discord::RouteOptions,
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
