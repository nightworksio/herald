use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator};

use crate::kinds::Kinds;

use crate::file::{
    AnnounceFile, AppFile, HttpFile, KeepFile, MarkersFile, RenderFile, RouteFile, ScheduleFile,
    ServerFile, TemplateFile,
};

/// `[store]`: one store, by its kind.
#[derive(JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
#[schemars(rename = "Store")]
#[allow(dead_code, reason = "read only by the schema's derive")]
enum StoreSchema {
    /// A SQLite file, on a volume or a persistent disk.
    Sqlite {
        #[serde(flatten)]
        options: herald_store_sqlite::StoreOptions,
        #[serde(flatten)]
        keep: KeepFile,
    },
    /// A Postgres database.
    Postgres {
        #[serde(flatten)]
        options: herald_store_postgres::StoreOptions,
        #[serde(flatten)]
        keep: KeepFile,
    },
    /// Cloudflare D1, through the Worker's binding.
    D1 {
        #[serde(flatten)]
        options: herald_store_d1::StoreOptions,
        #[serde(flatten)]
        keep: KeepFile,
    },
    /// The storage of herald's Cloudflare Durable Object, through the
    /// Worker's binding.
    DurableObject {
        #[serde(flatten)]
        options: herald_store_durable_object::StoreOptions,
        #[serde(flatten)]
        keep: KeepFile,
    },
    /// An AWS DynamoDB table.
    Dynamodb {
        #[serde(flatten)]
        options: herald_store_dynamodb::StoreOptions,
        #[serde(flatten)]
        keep: KeepFile,
    },
    /// Deno KV, on Deno Deploy or wherever a database is opened.
    DenoKv {
        #[serde(flatten)]
        options: herald_store_deno_kv::StoreOptions,
        #[serde(flatten)]
        keep: KeepFile,
    },
    /// A Fastly KV Store, on Fastly Compute.
    FastlyKv {
        #[serde(flatten)]
        options: herald_store_fastly_kv::StoreOptions,
        #[serde(flatten)]
        keep: KeepFile,
    },
    /// The destinations' own posts.
    Markers {},
    /// One file per record on a `herald-state` branch.
    GitBranch {
        #[serde(flatten)]
        options: herald_store_git_branch::StoreOptions,
        #[serde(flatten)]
        keep: KeepFile,
    },
    /// Generations of the GitHub Actions cache.
    ActionsCache {
        #[serde(flatten)]
        options: herald_store_actions_cache::StoreOptions,
        #[serde(flatten)]
        keep: KeepFile,
    },
    /// The repository's variables.
    RepoVariables(herald_store_repo_variables::StoreOptions),
}

/// A destination, by its kind: its schema is its kind's.
struct DestinationSchema;

impl JsonSchema for DestinationSchema {
    fn schema_name() -> Cow<'static, str> {
        DESTINATION.into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        Schema::default()
    }
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

/// A route: its schema is its kind's.
struct RouteSchema;

impl JsonSchema for RouteSchema {
    fn schema_name() -> Cow<'static, str> {
        ROUTE.into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        Schema::default()
    }
}

/// A destination's table, tagged by its kind.
#[derive(JsonSchema)]
#[serde(tag = "kind")]
#[allow(dead_code, reason = "read only by the schema's derive")]
enum Tagged<T> {
    #[serde(rename = "")]
    Kind(T),
}

/// A route to a destination of one kind.
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
#[allow(dead_code, reason = "read only by the schema's derive")]
struct Routed<T> {
    #[serde(flatten)]
    common: RouteFile,
    #[serde(flatten)]
    own: T,
}

/// The schema's name for a destination.
const DESTINATION: &str = "Destination";
/// The schema's name for a route.
const ROUTE: &str = "Route";

/// `schema`, its description first.
fn described(schema: &Schema, description: &str) -> Schema {
    let mut map = serde_json::Map::new();
    map.insert("description".to_owned(), description.into());
    map.extend(
        schema
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(key, _)| key.as_str() != "description")
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    Schema::from(map)
}

/// The schema of a destination of the kind `name`, with options `T`.
pub(crate) fn tagged<T: JsonSchema>(
    generator: &mut SchemaGenerator,
    name: &str,
    description: &str,
) -> Schema {
    let mut schema = Tagged::<T>::json_schema(generator);
    let mut variant = schema
        .get_mut("oneOf")
        .and_then(|variants| variants.as_array_mut())
        .and_then(Vec::pop)
        .and_then(|variant| Schema::try_from(variant).ok())
        .unwrap_or(schema);
    if let Some(kind) = variant.pointer_mut("/properties/kind/const") {
        *kind = name.into();
    }
    described(&variant, description)
}

/// The schema of a route to a destination of one kind, with options `T`,
/// kept as the definition `name` and referred to.
pub(crate) fn routed<T: JsonSchema>(
    generator: &mut SchemaGenerator,
    name: &str,
    description: &str,
) -> Schema {
    let schema = described(&Routed::<T>::json_schema(generator), description);
    generator
        .definitions_mut()
        .insert(name.to_owned(), schema.to_value());
    let mut reference = serde_json::Map::new();
    reference.insert("description".to_owned(), description.into());
    reference.insert("$ref".to_owned(), format!("#/$defs/{name}").into());
    Schema::from(reference)
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
    #[expect(
        clippy::zero_sized_map_values,
        reason = "only the schema's derive reads the map, and its kinds' schemas take the value's place"
    )]
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
    /// How `herald server` listens, and what it takes on at once.
    server: Option<ServerFile>,
    /// How herald's own requests reach the services the config names.
    http: Option<HttpFile>,
    /// How far a template may go as it renders.
    render: Option<RenderFile>,
    /// How a run holds and keeps an announcement.
    announce: Option<AnnounceFile>,
}

/// The JSON Schema of `herald.toml` with `kinds`, as
/// committed in `herald.schema.json`.
#[must_use]
pub fn schema(kinds: &Kinds) -> String {
    let mut generator = SchemaGenerator::default();
    let mut root = generator.root_schema_for::<FileSchema>();
    let destinations: Vec<_> = kinds
        .destinations()
        .map(|kind| kind.destination_schema(&mut generator).to_value())
        .collect();
    let routes: Vec<_> = kinds
        .destinations()
        .map(|kind| kind.route_schema(&mut generator).to_value())
        .collect();
    let mut definitions: BTreeMap<String, serde_json::Value> = generator
        .definitions()
        .iter()
        .map(|(name, schema)| (name.clone(), schema.clone()))
        .collect();
    definitions.insert(
        DESTINATION.to_owned(),
        serde_json::json!({ "description": "A destination, by its kind.", "oneOf": destinations }),
    );
    definitions.insert(
        ROUTE.to_owned(),
        serde_json::json!({
            "description": "A route: what it takes, where it announces, and its destination's own\noptions, as its destination's kind defines them.",
            "anyOf": routes,
        }),
    );
    root.insert(
        "$defs".to_owned(),
        serde_json::Value::Object(definitions.into_iter().collect()),
    );
    format!(
        "{}\n",
        serde_json::to_string_pretty(&root).unwrap_or_default()
    )
}
