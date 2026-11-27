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
        kinds: Vec::new(),
    };
    written.out.push_str(
        "# herald.toml settings\n\nEvery key `herald.toml` takes, generated from \
         [`herald.schema.json`](../herald.schema.json). Regenerate it with \
         `cargo run -p herald-compose --example settings > .docs/settings.md`.\n",
    );
    for (key, node) in properties(&root) {
        let mut rows = Vec::new();
        written.heading("##", &format!("`{key}`"), &said(node));
        written.walk(key, node, &mut rows, &mut Vec::new());
        written.table(&rows);
    }
    written.types();
    written.out.push_str(&crate::presets::reference());
    written.out
}

struct Written<'a> {
    defs: Option<&'a Map<String, Value>>,
    out: String,
    types: Vec<&'a str>,
    kinds: Vec<&'a str>,
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
                let what = said(variant);
                let kinded = kind(shape);
                if let Some(kind) = kinded {
                    self.heading("###", &format!("`{path}` of kind `{kind}`"), &what);
                } else {
                    let named = what.trim_end_matches('.');
                    let under = self
                        .kinds
                        .last()
                        .map_or_else(String::new, |kind| format!(" of kind `{kind}`"));
                    self.heading("###", &format!("`{path}`{under}: {named}"), "");
                }
                self.kinds.extend(kinded);
                let mut own = Vec::new();
                self.walk(path, variant, &mut own, within);
                self.table(&own);
                if kinded.is_some() {
                    self.kinds.pop();
                }
            }
        } else if let Some(fields) = resolved.get("properties").and_then(Value::as_object) {
            described(path, node, "table", rows);
            for (key, field) in fields {
                self.walk(&format!("{path}.{key}"), field, rows, within);
            }
        } else if let Some(entry) = resolved
            .get("additionalProperties")
            .filter(|entry| entry.is_object())
        {
            described(path, node, "table", rows);
            self.walk(&format!("{path}.<name>"), entry, rows, within);
        } else if let Some(item) = resolved.get("items").filter(|item| self.nested(item)) {
            described(path, node, "array of tables", rows);
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
        if node.get("properties").is_some() {
            return None;
        }
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
            self.heading("###", name, &described.map(said).unwrap_or_default());
        }
    }

    fn heading(&mut self, level: &str, heading: &str, what: &str) {
        let _ = writeln!(self.out, "\n{level} {heading}");
        if !what.is_empty() {
            let _ = writeln!(self.out, "\n{what}");
        }
    }
}

/// A row for the table or array of tables `node` at `path`, of `shape`,
/// where it is nested and says what it is.
fn described(path: &str, node: &Value, shape: &str, rows: &mut Vec<String>) {
    let what = said(node);
    if path.contains('.') && !what.is_empty() {
        rows.push(format!("| `{path}` | {shape} | {what} |"));
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

    const SCHEMA: &str = r##"{
        "properties": {
            "tree": {"description": "A tree.", "$ref": "#/$defs/Tree"},
            "loose": {"description": "Anything | at all."},
            "list": {"type": "array"},
            "names": {"type": "array", "items": {"type": "string"}},
            "rows": {"type": "array", "items": {"$ref": "#/$defs/Row"}},
            "nest": {"type": "object", "properties": {
                "cells": {"description": "Its cells.", "type": "array", "items": {"$ref": "#/$defs/Row"}},
                "named": {"description": "Its names.", "type": "object", "additionalProperties": {"type": "string"}}
            }},
            "shape": {"description": "A shape.", "$ref": "#/$defs/Shape"},
            "held": {"description": "A held table.", "$ref": "#/$defs/Held"},
            "named": {"type": "object", "additionalProperties": {"type": "boolean"}}
        },
        "$defs": {
            "Tree": {
                "type": "object",
                "properties": {
                    "child": {"description": "Its child.", "$ref": "#/$defs/Tree"},
                    "box": {"description": "A box.", "type": "object", "properties": {"lid": {"type": "boolean"}}},
                    "leaf": {"$ref": "#/$defs/Leaf"},
                    "bare": {"$ref": "#/$defs/Bare"}
                }
            },
            "Leaf": {"description": "A leaf.", "type": "string"},
            "Held": {
                "type": "object",
                "properties": {
                    "left": {"description": "Its left.", "type": "array", "items": {"type": "string"}},
                    "right": {"description": "Its right.", "type": "array", "items": {"type": "string"}}
                },
                "anyOf": [
                    {"required": ["left"], "properties": {"left": {"minItems": 1}}},
                    {"required": ["right"], "properties": {"right": {"minItems": 1}}}
                ]
            },
            "Bare": {"type": "string"},
            "Row": {"type": "object", "properties": {"cell": {"type": "integer"}}},
            "Shape": {
                "oneOf": [
                    {"description": "A circle.", "type": "object", "properties": {"kind": {"const": "circle"}, "radius": {"type": "number"}, "tint": {"oneOf": [{"description": "A solid fill.", "type": "object", "properties": {"color": {"type": "string"}}}, {"description": "No fill.", "type": "object", "properties": {"none": {"type": "boolean"}}}]}}},
                    {"description": "A free form.", "type": "object", "properties": {"points": {"type": "integer"}}},
                    {"const": "dot"}
                ]
            }
        }
    }"##;

    #[test]
    fn a_reference_follows_refs_once_and_writes_each_kind_and_type() {
        let written = reference(SCHEMA);
        for row in [
            "| `tree.box` | table | A box. |",
            "| `tree.box.lid` | boolean |  |",
            "| `tree.leaf` | [Leaf](#leaf) | A leaf. |",
            "| `loose` | any | Anything \\| at all. |",
            "| `list` | array |  |",
            "| `names` | array of string |  |",
            "| `rows[].cell` | integer |  |",
            "| `named.<name>` | boolean |  |",
            "\n### `shape` of kind `circle`\n\nA circle.\n\n| Setting",
            "| `shape.radius` | number |  |",
            "\n### `shape`: A free form\n\n| Setting",
            "\n### `shape.tint` of kind `circle`: A solid fill\n\n| Setting",
            "| `shape.tint.color` | string |  |",
            "\n### Bare\n\n### Leaf\n\nA leaf.\n",
            "| `held.left` | array of string | Its left. |",
            "| `nest.cells` | array of tables | Its cells. |",
            "| `nest.cells[].cell` | integer |  |",
            "| `nest.named` | table | Its names. |",
            "| `nest.named.<name>` | string |  |",
            "| `held.right` | array of string | Its right. |",
        ] {
            assert!(written.contains(row), "{row} in {written}");
        }
        assert!(!written.contains("tree.child"), "{written}");
        assert!(!written.contains("### `held`"), "{written}");
        assert!(!written.contains("\n\n\n"), "{written}");
        assert!(
            reference("not json").ends_with(&format!("## Types\n{}", crate::presets::reference()))
        );
    }
}
