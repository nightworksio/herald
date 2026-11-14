//! herald's app source (ADR-0003): an event an app writes as herald's event
//! JSON becomes an [`Event`], parsed without I/O. [`schema`] generates the
//! JSON Schema `herald-event.schema.json` holds.

use std::collections::BTreeMap;
use std::fmt;

use herald_core::{Attribute, Draft, Event, Kind, KindSpecs, Problem, Standing};
use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The namespace a store admits each app's event under, by its identity
/// (ADR-0013).
pub const ADMITTED: &str = "app";

/// One event as an app writes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "herald event")]
pub struct AppEvent {
    /// The app's own id for the event, which no other event of the app has.
    /// Any text: a request reading its standing back writes it as one path
    /// segment, percent-encoded.
    #[schemars(length(min = 1, max = MOST), pattern(WRITTEN))]
    pub id: String,
    /// What kind of thing it announces: one herald ships or one the config
    /// declares; `message` where it is left out.
    #[serde(default = "message")]
    pub kind: Kind,
    /// The project it is about.
    #[schemars(length(min = 1, max = MOST), pattern(WRITTEN))]
    pub project: String,
    /// A title, on one line.
    #[schemars(length(min = 1, max = MOST), pattern(WRITTEN))]
    pub title: String,
    /// The body, in Markdown.
    #[serde(default)]
    #[schemars(length(max = MOST))]
    pub body: String,
    /// Named links, each an `https` or `http` URL: `release`, `changelog`,
    /// `comparison`, `download`.
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, Link>", extend("propertyNames" = { "pattern": WRITTEN }))]
    pub links: BTreeMap<String, String>,
    /// Its kind's attributes, by name: a versioned kind's `version`, and
    /// `prerelease`.
    #[serde(default)]
    pub attributes: BTreeMap<String, Attribute>,
    /// When it happened, in RFC 3339; when herald read it where it is left
    /// out.
    #[serde(default)]
    #[schemars(with = "Option<String>", extend("format" = "date-time"))]
    pub occurred: Option<Timestamp>,
}

/// The pattern of a field herald refuses when it holds only white space.
const WRITTEN: &str = r"\S";

/// A link of an event, as the schema holds it: an `https` or `http` URL.
struct Link;

impl JsonSchema for Link {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Link".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "format": "uri",
            "pattern": "^https?://",
            "maxLength": MOST
        })
    }
}

/// The kind of an event an app leaves unnamed.
fn message() -> Kind {
    Kind::MESSAGE
}

/// Why an app's event could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnreadEvent {
    /// The bytes are not herald's event JSON.
    NotAnEvent(String),
    /// The event has no id.
    NoId,
    /// The event is longer than an app's event may be.
    TooLarge,
    /// A field of the event cannot be part of one.
    Problem(Problem),
    /// The config knows no kind of event by its name.
    UnknownKind(Kind),
    /// Its attributes are not what its kind's events carry, as the reason
    /// says.
    Unfit(String),
}

impl fmt::Display for UnreadEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnEvent(why) => write!(f, "the event is not herald's event JSON: {why}"),
            Self::NoId => f.write_str("the event has no id; an app gives every event one"),
            Self::TooLarge => write!(
                f,
                "the event is longer than {MOST} bytes; send a shorter one"
            ),
            Self::Problem(problem) => problem.fmt(f),
            Self::UnknownKind(kind) => write!(
                f,
                "herald knows no kind \"{kind}\"; send a kind it ships, or one the config declares under [kinds.{kind}]"
            ),
            Self::Unfit(reason) => f.write_str(reason),
        }
    }
}

/// The most bytes an app's event may hold (ADR-0003).
pub const MOST: usize = herald_wire::PARSE_LIMIT;

/// The identity of the event `app` gave `id`, white space around it taken
/// off.
#[must_use]
pub fn identity(app: &str, id: &str) -> String {
    format!("app/{app}/{}", id.trim())
}

/// The event `body` holds, written by the app named `app`, of one of the
/// kinds `kinds`, which happened at `now` where it says no time of its own.
///
/// # Errors
///
/// The [`UnreadEvent`] that keeps `body` from being an event.
pub fn event(
    app: &str,
    body: &[u8],
    now: Timestamp,
    kinds: &KindSpecs,
) -> Result<Event, UnreadEvent> {
    if body.len() > MOST {
        return Err(UnreadEvent::TooLarge);
    }
    let written: AppEvent =
        serde_json::from_slice(body).map_err(|error| UnreadEvent::NotAnEvent(error.to_string()))?;
    if written.id.trim().is_empty() {
        return Err(UnreadEvent::NoId);
    }
    let Some(spec) = kinds.get(&written.kind) else {
        return Err(UnreadEvent::UnknownKind(written.kind));
    };
    let event = Event::normalise(Draft {
        identity: identity(app, &written.id),
        kind: written.kind,
        project: written.project,
        title: written.title,
        body: written.body,
        links: written.links,
        attributes: written.attributes,
        occurred: written.occurred.unwrap_or(now),
    })
    .map_err(UnreadEvent::Problem)?;
    spec.check(event.attributes()).map_err(UnreadEvent::Unfit)?;
    Ok(event)
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
