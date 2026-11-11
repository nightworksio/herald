//! How a key a table leaves out is written, read from the schema of the type
//! the table is read into, for the error that names it.

use schemars::{JsonSchema, Schema};
use serde_json::Value;

/// How serde names a field a table leaves out.
const MISSING: &str = "missing field `";

/// `message` with how to write the key it says is missing, where it says
/// one is, read from the schema of `T`; `within` is the path from the table
/// read as `T` to the table that leaves the key out.
pub(crate) fn explained<T: JsonSchema>(message: &str, within: &[&str]) -> String {
    let Some(key) = message
        .strip_prefix(MISSING)
        .and_then(|rest| rest.strip_suffix('`'))
    else {
        return message.to_owned();
    };
    let root = schemars::schema_for!(T);
    let written = within
        .iter()
        .try_fold(root.as_value(), |schema, step| {
            property(&root, schema, step)
        })
        .and_then(|table| property(&root, table, key))
        .map_or_else(|| PLACEHOLDER.to_owned(), |schema| example(&root, schema));
    format!("{message}; give it, such as {key} = {written}")
}

/// How a value is written where its schema shows nothing better.
const PLACEHOLDER: &str = "\"…\"";

/// The schema `schema` refers to, where it is a reference into `root`.
fn resolved<'s>(root: &'s Schema, schema: &'s Value) -> &'s Value {
    schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix('#'))
        .and_then(|pointer| root.pointer(pointer))
        .unwrap_or(schema)
}

/// The schema of the key `key` of a table of the schema `schema`, looking
/// through references, the items of an array and each branch of a choice.
fn property<'s>(root: &'s Schema, schema: &'s Value, key: &str) -> Option<&'s Value> {
    let schema = resolved(root, schema);
    if let Some(found) = schema
        .get("properties")
        .and_then(|properties| properties.get(key))
    {
        return Some(found);
    }
    if let Some(items) = schema.get("items") {
        return property(root, items, key);
    }
    ["anyOf", "oneOf", "allOf"]
        .iter()
        .filter_map(|choice| schema.get(choice).and_then(Value::as_array))
        .flatten()
        .find_map(|branch| property(root, branch, key))
}

/// How a value of `schema` is written in TOML: its first example, or a
/// placeholder of its type.
fn example(root: &Schema, schema: &Value) -> String {
    let first = |schema: &Value| {
        schema
            .get("examples")
            .and_then(Value::as_array)
            .and_then(|examples| examples.first())
            .map(inline)
    };
    if let Some(written) = first(schema) {
        return written;
    }
    let schema = resolved(root, schema);
    if let Some(written) = first(schema) {
        return written;
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("array") => format!(
            "[{}]",
            schema
                .get("items")
                .map_or_else(|| PLACEHOLDER.to_owned(), |items| example(root, items))
        ),
        Some("object") => {
            let keys: Vec<String> = schema
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|key| {
                    let value = property(root, schema, key)
                        .map_or_else(|| PLACEHOLDER.to_owned(), |value| example(root, value));
                    format!("{key} = {value}")
                })
                .collect();
            format!("{{ {} }}", keys.join(", "))
        }
        Some("boolean") => "true".to_owned(),
        Some("integer" | "number") => "1".to_owned(),
        _ => PLACEHOLDER.to_owned(),
    }
}

/// `value` written as an inline TOML value.
fn inline(value: &Value) -> String {
    match value {
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(inline).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(keys) => format!(
            "{{ {} }}",
            keys.iter()
                .map(|(key, value)| format!("{key} = {}", inline(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(JsonSchema)]
    #[allow(dead_code, reason = "read only by the schema's derive")]
    struct Outer {
        #[schemars(example = Outer::REPOSITORY)]
        repository: String,
        named: Vec<String>,
        token: herald_core::SecretRef,
        inner: Inner,
        list: Vec<Inner>,
        choice: Option<Inner>,
        flag: bool,
        count: u8,
        anything: serde_json::Value,
    }

    impl Outer {
        const REPOSITORY: &str = "owner/name";
    }

    #[derive(JsonSchema)]
    #[allow(dead_code, reason = "read only by the schema's derive")]
    struct Inner {
        name: String,
        #[schemars(example = serde_json::json!({ "secret": "HERALD_TOKEN" }))]
        table: herald_core::SecretRef,
    }

    fn missing(key: &str, within: &[&str]) -> String {
        explained::<Outer>(&format!("missing field `{key}`"), within)
    }

    #[test]
    fn a_missing_key_is_named_with_how_it_is_written() {
        let cases = [
            ("repository", &[][..], "repository = \"owner/name\""),
            ("named", &[], "named = [\"…\"]"),
            ("token", &[], "token = { secret = \"NAME\" }"),
            ("flag", &[], "flag = true"),
            ("count", &[], "count = 1"),
            ("anything", &[], "anything = \"…\""),
            ("unknown", &[], "unknown = \"…\""),
            ("name", &["inner"], "name = \"…\""),
            ("name", &["list"], "name = \"…\""),
            ("name", &["choice"], "name = \"…\""),
            ("table", &["inner"], "table = { secret = \"HERALD_TOKEN\" }"),
            ("name", &["nowhere"], "name = \"…\""),
        ];
        for (key, within, written) in cases {
            assert_eq!(
                missing(key, within),
                format!("missing field `{key}`; give it, such as {written}"),
                "{key} in {within:?}"
            );
        }
    }

    #[test]
    fn another_message_is_kept_as_it_is() {
        assert_eq!(
            explained::<Outer>("unknown field `x`", &[]),
            "unknown field `x`"
        );
        assert_eq!(
            explained::<Outer>("missing field `x", &[]),
            "missing field `x"
        );
    }

    #[test]
    fn a_schema_that_shows_little_is_written_with_placeholders() {
        let root = schemars::schema_for!(Outer);
        let cases = [
            (serde_json::json!({ "type": "array" }), "[\"…\"]"),
            (
                serde_json::json!({ "type": "object", "required": ["gone"] }),
                "{ gone = \"…\" }",
            ),
            (serde_json::json!({ "$ref": "#/$defs/Nowhere" }), "\"…\""),
        ];
        for (schema, written) in cases {
            assert_eq!(example(&root, &schema), written, "{schema}");
        }
        let shown = Schema::try_from(serde_json::json!({
            "$defs": { "Shown": { "type": "string", "examples": ["shown"] } }
        }))
        .unwrap();
        assert_eq!(
            example(&shown, &serde_json::json!({ "$ref": "#/$defs/Shown" })),
            "\"shown\""
        );
    }

    #[test]
    fn a_value_is_written_inline() {
        assert_eq!(
            inline(&serde_json::json!({ "secret": "NAME", "n": [1, true] })),
            "{ n = [1, true], secret = \"NAME\" }"
        );
    }
}
