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
    /// the value the preset's tables write, or where they write none, the
    /// value the key takes where absent.
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
    /// Each of its views.
    Views,
    /// Each of its rules.
    Rules,
    /// Each of its merge queues.
    MergeQueues,
}

impl Lands {
    /// The section of `herald.toml` the tables are written under.
    #[must_use]
    pub const fn section(self) -> &'static str {
        match self {
            Self::Routes => "routes",
            Self::Templates => "templates",
            Self::Views => "views",
            Self::Rules => "rules",
            Self::MergeQueues => "merge_queues",
        }
    }
}

/// The prefix of a shipped preset's name.
pub const SHIPPED_PREFIX: &str = "herald:";

/// The presets herald ships, by name.
pub const SHIPPED: &[Preset] = &[
    ANNOUNCE_RELEASES,
    REVIEW_REMINDERS,
    MERGE_QUEUE_SMALL_TEAM,
    SECURITY_ALERTS_PRIVATE,
];

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

fn lengths(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<Vec<herald_core::Duration>>()
}

fn days(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<Vec<crate::views::DayFile>>()
}

fn runs(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<herald_core::merging::Runs>()
}

fn method(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<herald_core::merging::Method>()
}

fn enter(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<herald_core::merging::Enter>()
}

fn write(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<herald_core::merging::Write>()
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

const TITLE: Parameter = Parameter {
    key: "title",
    described: "The template of each announcement's title.",
    required: false,
    lands: Lands::Templates,
    schema: name,
};

const BODY: Parameter = Parameter {
    key: "body",
    described: "The template of each announcement's body.",
    required: false,
    lands: Lands::Templates,
    schema: name,
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
        TITLE,
        BODY,
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

const REVIEW_REMINDERS: Preset = Preset {
    name: "herald:review-reminders",
    described: "Each open pull request waiting on review past each of its stages, nudged at 09:00 on weekdays to its destination.",
    parameters: &[
        DESTINATION,
        Parameter {
            key: "sources",
            described: "The sources whose open pull requests it watches, as `[sources]` names them; every forge source where not given.",
            required: false,
            lands: Lands::Views,
            schema: names,
        },
        Parameter {
            key: "stages",
            described: "How long a pull request waits on review, since anyone last acted on it, before each nudge.",
            required: false,
            lands: Lands::Rules,
            schema: lengths,
        },
        Parameter {
            key: "at",
            described: "The wall-clock time, as \"HH:MM\", it nudges at.",
            required: false,
            lands: Lands::Rules,
            schema: name,
        },
        Parameter {
            key: "days",
            described: "The days it nudges on.",
            required: false,
            lands: Lands::Rules,
            schema: days,
        },
        Parameter {
            key: "zone",
            described: "The IANA time zone `at` and `days` are read in, such as \"Europe/Amsterdam\".",
            required: false,
            lands: Lands::Rules,
            schema: name,
        },
    ],
    tables: r#"[[routes]]
id = "reminders"
sources = ["reviews"]
template = "herald:nudge"

[views.reviews]
states = ["waiting-on-review"]

[rules.reviews]
view = "reviews"
stages = ["2d", "7d"]
at = "09:00"
days = ["mon", "tue", "wed", "thu", "fri"]
zone = "UTC"
"#,
};

const MERGE_QUEUE_SMALL_TEAM: Preset = Preset {
    name: "herald:merge-queue-small-team",
    described: "A merge queue on one base branch, which a writer enters a pull request into by a label or a comment.",
    parameters: &[
        Parameter {
            key: "source",
            described: "The source whose repository it merges in, of kind `github`, `gitlab` or `forgejo`, as `[sources]` names it.",
            required: true,
            lands: Lands::MergeQueues,
            schema: name,
        },
        Parameter {
            key: "branch",
            described: "The base branch it merges into, such as `main`.",
            required: true,
            lands: Lands::MergeQueues,
            schema: name,
        },
        Parameter {
            key: "method",
            described: "How it merges: `merge`, `squash`, `rebase`, `fast-forward`, or `repository`, the repository's one allowed method.",
            required: true,
            lands: Lands::MergeQueues,
            schema: method,
        },
        Parameter {
            key: "write",
            described: "The credential its steps are made with, held apart from every one herald reads with: `{ token = { secret = \"NAME\" } }` or `{ app = \"<github_apps name>\" }`.",
            required: true,
            lands: Lands::MergeQueues,
            schema: write,
        },
        Parameter {
            key: "runs",
            described: "Who runs it: `forge`, the forge's own merge queue or merge train, where not given; or `herald`, herald's own serial queue.",
            required: false,
            lands: Lands::MergeQueues,
            schema: runs,
        },
        Parameter {
            key: "enter",
            described: "The gesture that enters a pull request: `{ label = \"herald:merge\" }` where not given, or another label or a comment's command.",
            required: false,
            lands: Lands::MergeQueues,
            schema: enter,
        },
    ],
    tables: "[merge_queues.queue]\n",
};

const SECURITY_ALERTS_PRIVATE: Preset = Preset {
    name: "herald:security-alerts-private",
    described: "Each security alert of its sources, announced to its destination, which its route states is private.",
    parameters: &[DESTINATION, SOURCES, TITLE, BODY],
    tables: r#"[[routes]]
id = "alerts"
kinds = ["alert"]
private = true
template = "alert"

[templates.alert]
title = "{{ event.project }}: {{ event.title }}"
body = "{% if event.attributes.severity %}Severity: {{ event.attributes.severity }}.{% endif %}"
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
                "optional"
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

#[cfg(test)]
mod tests {
    use super::{MERGE_QUEUE_SMALL_TEAM, Parameter};

    #[test]
    fn the_merge_queue_s_gesture_is_said_to_be_the_label_a_queue_takes_where_none_is_given() {
        let enter: &Parameter = MERGE_QUEUE_SMALL_TEAM
            .parameters
            .iter()
            .find(|parameter| parameter.key == "enter")
            .unwrap();
        assert!(
            enter.described.contains(&format!(
                "{{ label = \"{}\" }}",
                herald_core::merging::ENTER_LABEL
            )),
            "{}",
            enter.described
        );
    }
}
