use std::fmt;

use herald_core::{
    Announcement, Bullet, Emphases, Escape, Facts, Features, Fences, Heading, Limits, Links,
    Markup, Measure, MentionStyle, Pins, Profile, Sent, Step, Symbols, Threads, Waits,
};
use herald_ports::{Base, Http, HttpError, Response, Secret};
use serde_json::Value;
use url::Url;

use crate::request::request;

/// Card Markdown, as an Adaptive Card's text block reads it in Teams: text
/// escaped by backslashes, each link shown under its own target, a heading
/// written bold, code as plain text, no strike and no HTML (ADR-0093;
/// <https://learn.microsoft.com/en-us/microsoftteams/platform/task-modules-and-cards/cards/cards-format>).
pub const MARKUP: Markup = Markup {
    escape: Escape::Backslash,
    links: Links::Shown,
    heading: Heading::Wrapped("**"),
    marks: Emphases {
        emphasis: "_",
        strong: "**",
        strike: "",
    },
    fences: Fences::None,
    bullet: Bullet::Fixed("- "),
    html: false,
    pills: None,
};

/// The most bytes a message to a Workflows webhook holds: 28 KB, read as
/// thousands of bytes, the smaller reading
/// (<https://learn.microsoft.com/en-us/microsoftteams/platform/webhooks-and-connectors/how-to/add-incoming-webhook>).
pub const MESSAGE: usize = 28_000;

/// The longest title, in bytes.
const TITLE: usize = 1_000;

/// The longest text of a card, in bytes: room for its escapes to grow
/// threefold as JSON writes them, beside its title, envelope and buttons,
/// within [`MESSAGE`].
const TEXT: usize = 8_000;

/// The platform, as herald names it.
pub(crate) const TEAMS: &str = "Microsoft Teams";

/// Why herald cannot check who may read where a Workflows webhook posts.
pub const UNCHECKED: &str = "herald cannot ask Microsoft Teams who may read the channel or chat a Workflows webhook posts to; say vouch_unchecked = true on the route where only those meant to may read it";

/// Why a webhook's URL does not take a request.
pub(crate) fn unaddressed(webhook: &Secret) -> String {
    format!(
        "the secret {} holds no HTTPS URL; give it the URL the workflow's trigger, \"When a Teams webhook request is received\", shows",
        webhook.name()
    )
}

/// One Microsoft Teams destination, for one route. Its debug form names
/// the webhook's secret alone, never the origin read from its URL.
#[derive(Clone, PartialEq, Eq)]
pub struct Teams {
    pub(crate) webhook: Secret,
    origin: Option<String>,
    waits: Waits,
}

impl fmt::Debug for Teams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Teams")
            .field("webhook", &self.webhook)
            .field("waits", &self.waits)
            .finish_non_exhaustive()
    }
}

impl Teams {
    /// The destination posting through the Workflows webhook whose URL
    /// `webhook` holds.
    #[must_use]
    pub fn new(webhook: Secret) -> Self {
        let origin = Url::parse(webhook.expose())
            .ok()
            .filter(|url| url.scheme() == "https" && url.host_str().is_some())
            .map(|url| url.origin().ascii_serialization());
        Self {
            webhook,
            origin,
            waits: Waits::DEFAULT,
        }
    }

    /// The destination, waiting `waits` where Teams names no wait.
    #[must_use]
    pub fn waiting(self, waits: Waits) -> Self {
        Self { waits, ..self }
    }

    /// What its `Http` may reach: the HTTPS origin of its webhook's URL
    /// alone, or nothing where the secret holds no such URL (ADR-0007).
    #[must_use]
    pub fn base(&self) -> Base<'_> {
        self.origin.as_deref().map_or(Base::Among(&[]), Base::At)
    }

    /// Whether its webhook's secret holds an HTTPS URL.
    #[must_use]
    pub fn addressed(&self) -> bool {
        self.origin.is_some()
    }

    /// Sends `step` of `announcement` and reads Teams' answer.
    ///
    /// # Errors
    ///
    /// When the request got no response at all.
    pub async fn send<H: Http>(
        &self,
        http: &H,
        step: &Step,
        announcement: &Announcement,
    ) -> Result<Sent, HttpError> {
        match request(self, step, announcement) {
            Ok(request) => Ok(read(&http.send(request).await?, self.waits)),
            Err(refusal) => Ok(Sent::Refused(refusal)),
        }
    }
}

/// What Teams said in `response`: its error's code and message, where it
/// answered JSON that names them, or else its text, cut short.
fn said(response: &Response) -> String {
    const QUOTED: usize = 200;
    let body = serde_json::from_slice::<Value>(&response.body).ok();
    let error = body.as_ref().and_then(|body| body.get("error"));
    let field = |name| {
        error
            .and_then(|error| error.get(name))
            .and_then(Value::as_str)
    };
    let said = match (field("code"), field("message")) {
        (Some(code), Some(message)) => format!("{code}: {message}"),
        (Some(said), None) | (None, Some(said)) => said.to_owned(),
        (None, None) => String::from_utf8_lossy(&response.body).into_owned(),
    };
    said.trim().chars().take(QUOTED).collect()
}

/// What a post answers: landed, with nothing more to record, where Teams
/// took it; a wait, or why Teams refused it, otherwise.
fn read(response: &Response, waits: Waits) -> Sent {
    response
        .failure(waits, None, TEAMS, || said(response))
        .unwrap_or_else(|| Sent::Landed(Facts::default()))
}

/// What a Microsoft Teams destination can do through a Workflows webhook:
/// post, and nothing more.
#[must_use]
pub const fn profile() -> Profile {
    Profile {
        markup: MARKUP,
        measure: Measure::Bytes,
        limits: Limits {
            title: TITLE,
            text: TEXT,
            followup: TEXT,
            thread_name: TITLE,
        },
        features: Features {
            threads: Threads::None,
            crosspost: false,
            pins: Pins::None,
            reads_back: false,
            edits: false,
            mentions: MentionStyle::None,
            interactions: false,
        },
        symbols: Symbols::DEFAULT,
    }
}
