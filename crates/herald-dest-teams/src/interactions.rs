//! How a Teams bot takes what Teams posts to it, as
//! `[destinations.*.interactions]` writes it (ADR-0016, ADR-0093).

use schemars::JsonSchema;
use serde::Deserialize;

/// How Teams carries what it posts to a bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(rename = "TeamsVia")]
pub enum Via {
    /// Teams posts each activity to herald's endpoint with a bearer token
    /// the Bot Framework signs: served by `herald server` and by a
    /// serverless driver.
    #[default]
    Http,
}

/// How a Teams bot takes what Teams posts to it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "TeamsInteractions")]
pub struct Interactions {
    /// How Teams carries it.
    #[serde(default)]
    pub via: Via,
}
