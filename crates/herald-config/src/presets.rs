//! The presets herald ships (ADR-0092): each a few tables of `herald.toml`
//! a `[presets.<name>]` table adopts by its name, and the parameters it
//! takes.

use std::fmt::Write as _;

use schemars::{JsonSchema, Schema, SchemaGenerator};

/// A preset herald ships.
#[derive(Debug, Clone, Copy)]
pub struct Preset {
    /// The name `use` gives it, such as `herald:announce-releases`.
    pub name: &'static str,
    /// What it announces.
    pub described: &'static str,
    /// The keys an adopting table gives it.
    pub parameters: &'static [Parameter],
    /// The tables it expands to, as `herald.toml` writes them, each named
    /// as the preset names it before the adopting table's name is put in
    /// front.
    pub tables: &'static str,
}

/// A key a preset takes from the table that adopts it.
#[derive(Debug, Clone, Copy)]
pub struct Parameter {
    /// The key.
    pub key: &'static str,
    /// What it sets.
    pub described: &'static str,
    /// Whether the adopting table must give it; one it need not give takes
    /// the value the preset's tables write.
    pub required: bool,
    /// The tables of the preset it is written into.
    pub lands: Lands,
    /// The JSON Schema of its value.
    pub schema: fn(&mut SchemaGenerator) -> Schema,
}

/// The tables of a preset a key is written into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lands {
    /// Each of its routes.
    Routes,
    /// Each of its templates.
    Templates,
}

impl Lands {
    /// The section of `herald.toml` the tables are written under.
    #[must_use]
    pub const fn section(self) -> &'static str {
        match self {
            Self::Routes => "routes",
            Self::Templates => "templates",
        }
    }
}

/// The prefix of a shipped preset's name.
pub const SHIPPED_PREFIX: &str = "herald:";

/// The presets herald ships, by name.
pub const SHIPPED: &[Preset] = &[ANNOUNCE_RELEASES];

/// The preset shipped as `name`, where herald ships one.
#[must_use]
pub fn shipped(name: &str) -> Option<&'static Preset> {
    SHIPPED.iter().find(|preset| preset.name == name)
}

fn name(generator: &mut SchemaGenerator) -> Schema {
    String::json_schema(generator)
}

fn names(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<Vec<String>>()
}

fn prereleases(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<herald_core::Prereleases>()
}

const DESTINATION: Parameter = Parameter {
    key: "destination",
    described: "The destination its routes announce to, as `[destinations]` names it.",
    required: true,
    lands: Lands::Routes,
    schema: name,
};

const SOURCES: Parameter = Parameter {
    key: "sources",
    described: "The sources whose events its routes take, as `[sources]` names them.",
    required: true,
    lands: Lands::Routes,
    schema: names,
};

const ANNOUNCE_RELEASES: Preset = Preset {
    name: "herald:announce-releases",
    described: "Each release of its sources, announced once to its destination with its notes.",
    parameters: &[
        DESTINATION,
        SOURCES,
        Parameter {
            key: "prereleases",
            described: "Which pre-releases it announces: `include`, `exclude` or `only`.",
            required: false,
            lands: Lands::Routes,
            schema: prereleases,
        },
        Parameter {
            key: "title",
            described: "The template of each announcement's title.",
            required: false,
            lands: Lands::Templates,
            schema: name,
        },
        Parameter {
            key: "body",
            described: "The template of each announcement's body.",
            required: false,
            lands: Lands::Templates,
            schema: name,
        },
    ],
    tables: r#"[[routes]]
id = "releases"
kinds = ["release"]
prereleases = "include"
template = "release"

[templates.release]
title = "{{ event.project }} {{ event.attributes.version }}{% if event.attributes.prerelease %} · pre-release{% endif %}"
body = "{{ event.body }}"
"#,
};

/// The settings reference's section on the presets herald ships: each
/// one's parameters and the tables it expands to.
pub(crate) fn reference() -> String {
    let mut out = String::from(
        "\n## Shipped presets\n\nEach preset herald ships, the parameters a `[presets.<name>]` table gives it, and the tables it expands to, each named `<name>-` and then as written here.\n",
    );
    for preset in SHIPPED {
        let _ = write!(
            out,
            "\n### `{}`\n\n{}\n\n| Parameter | Given | What it sets |\n|---|---|---|\n",
            preset.name, preset.described
        );
        for parameter in preset.parameters {
            let given = if parameter.required {
                "always"
            } else {
                "or as written below"
            };
            let _ = writeln!(
                out,
                "| `{}` | {given} | {} |",
                parameter.key, parameter.described
            );
        }
        let _ = write!(out, "\n```toml\n{}```\n", preset.tables);
    }
    out
}
