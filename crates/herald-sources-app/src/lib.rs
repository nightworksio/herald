//! herald's app source (ADR-0003): an event an app writes as herald's event
//! JSON becomes an [`Event`], parsed without I/O. [`schema`] generates the
//! JSON Schema `herald-event.schema.json` holds.

use std::collections::BTreeMap;
use std::fmt;

use herald_core::{Draft, Event, Kind, Problem, Standing};
use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One event as an app writes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "herald event")]
pub struct AppEvent {
    /// The app's own id for the event, which no other event of the app has.
    pub id: String,
    /// The project it is about.
    pub project: String,
    /// The version it announces, where it announces one.
    #[serde(default)]
    pub version: Option<String>,
    /// A title, on one line.
    pub title: String,
    /// The body, in Markdown.
    #[serde(default)]
    pub body: String,
    /// Named links, each an `https` or `http` URL: `release`, `changelog`,
    /// `comparison`, `download`.
    #[serde(default)]
    pub links: BTreeMap<String, String>,
    /// Whether it is a pre-release.
    #[serde(default)]
    pub prerelease: bool,
    /// When it happened, in RFC 3339; when herald read it where it is left
    /// out.
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub occurred: Option<Timestamp>,
}

/// Why an app's event could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unread {
    /// The bytes are not herald's event JSON.
    NotAnEvent(String),
    /// The event has no id.
    NoId,
    /// A field of the event cannot be part of one.
    Problem(Problem),
}

impl fmt::Display for Unread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnEvent(why) => write!(f, "the event is not herald's event JSON: {why}"),
            Self::NoId => f.write_str("the event has no id; an app gives every event one"),
            Self::Problem(problem) => problem.fmt(f),
        }
    }
}

/// The identity of the event `app` gave `id`.
#[must_use]
pub fn identity(app: &str, id: &str) -> String {
    format!("app/{app}/{id}")
}

/// The event `body` holds, written by the app named `app`, which happened
/// at `now` where it says no time of its own.
///
/// # Errors
///
/// The [`Unread`] that keeps `body` from being an event.
pub fn event(app: &str, body: &[u8], now: Timestamp) -> Result<Event, Unread> {
    let written: AppEvent =
        serde_json::from_slice(body).map_err(|error| Unread::NotAnEvent(error.to_string()))?;
    let id = written.id.trim();
    if id.is_empty() {
        return Err(Unread::NoId);
    }
    Event::normalise(Draft {
        identity: identity(app, id),
        kind: Kind::Message,
        project: written.project,
        version: written.version,
        title: written.title,
        body: written.body,
        links: written.links,
        prerelease: written.prerelease,
        occurred: written.occurred.unwrap_or(now),
    })
    .map_err(Unread::Problem)
}

/// Where an app's event stands on each route it may go to, as the HTTP API
/// answers it.
#[derive(Debug, Serialize)]
struct Stands<'a> {
    id: &'a str,
    project: &'a str,
    routes: Vec<RouteStands<'a>>,
}

/// Where an app's event stands on one route.
#[derive(Debug, Serialize)]
struct RouteStands<'a> {
    route: &'a str,
    #[serde(flatten)]
    standing: &'a Standing,
}

/// The HTTP API's answer to where the event `id` of `project` stands on
/// each of `routes`, as JSON.
#[must_use]
pub fn standings(id: &str, project: &str, routes: &[(&str, Standing)]) -> String {
    let stands = Stands {
        id,
        project,
        routes: routes
            .iter()
            .map(|(route, standing)| RouteStands { route, standing })
            .collect(),
    };
    serde_json::to_string(&stands).unwrap_or_default()
}

/// The JSON Schema of an app's event, as committed in
/// `herald-event.schema.json`.
#[must_use]
pub fn schema() -> String {
    let schema = schemars::schema_for!(AppEvent);
    format!(
        "{}\n",
        serde_json::to_string_pretty(&schema).unwrap_or_default()
    )
}
