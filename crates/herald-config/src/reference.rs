use std::fmt::Write as _;

use serde_json::{Map, Value};

const REF_PREFIX: &str = "#/$defs/";

/// The settings reference `.docs/settings.md` holds: every key of
/// `herald.toml` the JSON Schema `schema` describes, by section and by kind,
/// with its type and what it sets.
#[must_use]
pub fn reference(schema: &str) -> String {
    let root: Value = serde_json::from_str(schema).unwrap_or_default();
    let defs = root.get("$defs").and_then(Value::as_object);
    let mut written = Written {
        defs,
        out: String::new(),
        types: Vec::new(),
    };
    written.out.push_str(
        "# herald.toml settings\n\nEvery key `herald.toml` takes, generated from \
         [`herald.schema.json`](../herald.schema.json). Regenerate it with \
         `cargo run -p herald-compose --example settings > .docs/settings.md`.\n",
    );
    for (key, node) in properties(&root) {
        let mut rows = Vec::new();
        let _ = write!(written.out, "\n## `{key}`\n\n{}\n", said(node));
        written.walk(key, node, &mut rows, &mut Vec::new());
        written.table(&rows);
    }
    written.types();
    written.out
}

struct Written<'a> {
    defs: Option<&'a Map<String, Value>>,
    out: String,
    types: Vec<&'a str>,
}

impl<'a> Written<'a> {
    fn walk(
        &mut self,
        path: &str,
        node: &'a Value,
        rows: &mut Vec<String>,
        within: &mut Vec<&'a str>,
    ) {
        let (named, resolved) = self.resolve(node);
        if named.is_some_and(|name| within.contains(&name)) {
            return;
        }
        within.extend(named);
        if let Some(variants) = self.variants(resolved) {
            self.table(rows);
            rows.clear();
            for variant in variants {
                let (_, shape) = self.resolve(variant);
                let _ = match kind(shape) {
                    Some(kind) => write!(
                        self.out,
                        "\n### `{path}` of kind `{kind}`\n\n{}\n",
                        said(variant)
                    ),
                    None => write!(self.out, "\n### `{path}`: {}\n", said(variant)),
                };
                let mut own = Vec::new();
                self.walk(path, variant, &mut own, within);
                self.table(&own);
            }
        } else if let Some(fields) = resolved.get("properties").and_then(Value::as_object) {
            let what = said(node);
            if path.contains('.') && !what.is_empty() {
                rows.push(format!("| `{path}` | table | {what} |"));
            }
            for (key, field) in fields {
                self.walk(&format!("{path}.{key}"), field, rows, within);
            }
        } else if let Some(entry) = resolved
            .get("additionalProperties")
            .filter(|entry| entry.is_object())
        {
            self.walk(&format!("{path}.<name>"), entry, rows, within);
        } else if let Some(item) = resolved.get("items").filter(|item| self.nested(item)) {
            self.walk(&format!("{path}[]"), item, rows, within);
        } else {
            let what = Some(said(node))
                .filter(|what| !what.is_empty())
                .unwrap_or_else(|| said(resolved));
            rows.push(format!("| `{path}` | {} | {what} |", self.typed(node)));
        }
        if named.is_some() {
            within.pop();
        }
    }

    fn resolve(&self, node: &'a Value) -> (Option<&'a str>, &'a Value) {
        let named = node
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|reference| reference.strip_prefix(REF_PREFIX));
        let resolved = named.and_then(|name| self.defs?.get(name)).unwrap_or(node);
        (named, resolved)
    }

    fn nested(&self, node: &'a Value) -> bool {
        let (_, resolved) = self.resolve(node);
        resolved.get("properties").is_some() || self.variants(resolved).is_some()
    }

    fn variants(&self, node: &'a Value) -> Option<&'a Vec<Value>> {
        node.get("oneOf")
            .or_else(|| node.get("anyOf"))
            .and_then(Value::as_array)
            .filter(|variants| {
                variants
                    .iter()
                    .any(|variant| self.resolve(variant).1.get("properties").is_some())
            })
    }

    fn typed(&mut self, node: &'a Value) -> String {
        if let (Some(name), _) = self.resolve(node) {
            if !self.types.contains(&name) {
                self.types.push(name);
            }
            return format!("[{name}](#{})", name.to_lowercase());
        }
        match node.get("type").and_then(Value::as_str) {
            Some("array") => node.get("items").map_or_else(
                || "array".to_owned(),
                |item| format!("array of {}", self.typed(item)),
            ),
            Some(kind) => kind.to_owned(),
            None => "any".to_owned(),
        }
    }

    fn table(&mut self, rows: &[String]) {
        if rows.is_empty() {
            return;
        }
        self.out
            .push_str("\n| Setting | Type | What it sets |\n|---------|------|--------------|\n");
        for row in rows {
            self.out.push_str(row);
            self.out.push('\n');
        }
    }

    fn types(&mut self) {
        let mut types = self.types.clone();
        types.sort_unstable();
        self.out.push_str("\n## Types\n");
        for name in types {
            let described = self.defs.and_then(|defs| defs.get(name));
            let _ = write!(
                self.out,
                "\n### {name}\n\n{}\n",
                described.map(said).unwrap_or_default()
            );
        }
    }
}

fn properties(node: &Value) -> impl Iterator<Item = (&String, &Value)> {
    node.get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
}

fn kind(node: &Value) -> Option<&str> {
    node.pointer("/properties/kind/const")?.as_str()
}

fn said(node: &Value) -> String {
    node.get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_follows_refs_once_and_names_what_it_cannot_type() {
        let schema = r##"{
            "properties": {
                "tree": {"description": "A tree.", "$ref": "#/$defs/Tree"},
                "loose": {"description": "Anything | at all."},
                "list": {"type": "array"}
            },
            "$defs": {
                "Tree": {
                    "type": "object",
                    "properties": {
                        "child": {"description": "Its child.", "$ref": "#/$defs/Tree"},
                        "box": {"description": "A box.", "type": "object", "properties": {"lid": {"type": "boolean"}}},
                        "leaf": {"$ref": "#/$defs/Leaf"}
                    }
                },
                "Leaf": {"description": "A leaf.", "type": "string"}
            }
        }"##;
        let written = reference(schema);
        assert!(
            written.contains("| `tree.leaf` | [Leaf](#leaf) | A leaf. |"),
            "{written}"
        );
        assert!(!written.contains("tree.child"), "{written}");
        assert!(
            written.contains("| `loose` | any | Anything \\| at all. |"),
            "{written}"
        );
        assert!(written.contains("| `list` | array |  |"), "{written}");
        assert!(written.ends_with("### Leaf\n\nA leaf.\n"), "{written}");
        assert!(reference("not json").ends_with("## Types\n"));
    }
}
