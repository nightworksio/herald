use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator};

use crate::config::Config;
use crate::kinds::Kinds;

use crate::file::{
    AnnounceFile, AppFile, HttpFile, KeepFile, KindFile, MarkersFile, RenderFile, RouteFile,
    ScheduleFile, ServerFile, TemplateFile,
};

/// `version`: the one version of the config this herald reads.
struct VersionSchema;

impl JsonSchema for VersionSchema {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "Version".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({ "type": "integer", "const": Config::VERSION })
    }
}

/// `[store]`: one store, by its kind: its schema is its kind's.
struct StoreSchema;

impl JsonSchema for StoreSchema {
    fn schema_name() -> Cow<'static, str> {
        STORE.into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        Schema::default()
    }
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

/// A source, by its kind: its schema is its kind's.
struct SourceSchema;

impl JsonSchema for SourceSchema {
    fn schema_name() -> Cow<'static, str> {
        SOURCE.into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        Schema::default()
    }
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

/// A store's options with how long it keeps its records.
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
#[allow(dead_code, reason = "read only by the schema's derive")]
struct Retained<T> {
    #[serde(flatten)]
    options: T,
    #[serde(flatten)]
    keep: KeepFile,
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

/// The schema's name for a source.
const SOURCE: &str = "Source";
/// The schema's name for a store.
const STORE: &str = "Store";
/// The schema's name for a destination.
const DESTINATION: &str = "Destination";
/// The schema's name for a route.
const ROUTE: &str = "Route";

/// `schema` with null taken out of every type it allows, as TOML has no
/// null: a key left out is how a config gives none.
fn unnulled(schema: &mut Schema) {
    if let Some(types) = schema
        .get_mut("type")
        .and_then(|types| types.as_array_mut())
    {
        types.retain(|kind| kind != "null");
        if let [only] = types.as_slice() {
            let only = only.clone();
            schema.insert("type".to_owned(), only);
        }
    }
    if let Some(any) = schema.get_mut("anyOf").and_then(|any| any.as_array_mut()) {
        any.retain(|branch| branch.get("type").is_none_or(|kind| kind != "null"));
        if let [only] = any.as_slice() {
            let only = only.clone();
            schema.remove("anyOf");
            if let Some(object) = only.as_object() {
                for (key, value) in object {
                    schema.insert(key.clone(), value.clone());
                }
            }
        }
    }
    if schema
        .get("default")
        .is_some_and(serde_json::Value::is_null)
    {
        schema.remove("default");
    }
    schemars::transform::transform_subschemas(&mut unnulled, schema);
}

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

/// The schema of a store of the kind `name`, with options `T`, and how long
/// it keeps its records where it is `retained`.
pub(crate) fn store<T: JsonSchema>(
    generator: &mut SchemaGenerator,
    name: &str,
    description: &str,
    retained: bool,
) -> Schema {
    if retained {
        tagged::<Retained<T>>(generator, name, description)
    } else {
        tagged::<T>(generator, name, description)
    }
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
    version: VersionSchema,
    /// The kinds of event the config declares beside those herald ships, by
    /// name; its apps may announce them.
    #[serde(default)]
    kinds: BTreeMap<String, KindFile>,
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
    #[expect(
        clippy::zero_sized_map_values,
        reason = "only the schema's derive reads the map, and its kinds' schemas take the value's place"
    )]
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
    let sources: Vec<_> = kinds
        .sources()
        .map(|kind| kind.schema(&mut generator).to_value())
        .collect();
    let stores: Vec<_> = kinds
        .stores()
        .map(|kind| kind.schema(&mut generator).to_value())
        .collect();
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
        SOURCE.to_owned(),
        serde_json::json!({ "description": "A source, by its kind.", "oneOf": sources }),
    );
    definitions.insert(
        STORE.to_owned(),
        serde_json::json!({ "description": "`[store]`: one store, by its kind.", "oneOf": stores }),
    );
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
    unnulled(&mut root);
    format!(
        "{}\n",
        serde_json::to_string_pretty(&root).unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(JsonSchema)]
    #[schemars(deny_unknown_fields)]
    #[allow(dead_code, reason = "read only by the schema's derive")]
    struct Options {
        path: String,
    }

    #[test]
    fn a_store_lists_how_long_it_keeps_records_only_where_it_is_retained() {
        let mut generator = SchemaGenerator::default();
        for retained in [true, false] {
            let schema = store::<Options>(&mut generator, "kept", "Kept.", retained);
            assert_eq!(
                schema.pointer("/properties/kind/const"),
                Some(&"kept".into())
            );
            assert_eq!(schema.pointer("/description"), Some(&"Kept.".into()));
            assert!(schema.pointer("/properties/path").is_some());
            assert_eq!(
                schema.pointer("/properties/keep_ledger").is_some(),
                retained
            );
        }
    }
}
