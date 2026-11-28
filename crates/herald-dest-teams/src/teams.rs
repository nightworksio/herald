use std::fmt;

use herald_core::{
    Announcement, Bullet, Emphases, Entry, Escape, Facts, Features, Fences, Heading, Limits, Links,
    Markup, Measure, MentionRoom, MentionStyle, Pins, Profile, Sent, Step, Symbols, Threads, Waits,
};
use herald_ports::{Base, Http, HttpError, Response, Secret};
use jiff::Timestamp;
use serde_json::Value;
use url::Url;

use crate::bot::Bot;
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

/// The longest text of a webhook's card, in bytes: room for its escapes to
/// grow threefold as JSON writes them, beside its title, envelope and
/// buttons, within [`MESSAGE`].
const TEXT: usize = 8_000;

/// The longest text of a bot's card, in bytes: room for its escapes to
/// grow threefold as JSON writes them and twofold again as UTF-16 counts
/// them, beside its title, envelope and buttons, within
/// [`crate::BOT_MESSAGE`].
const BOT_TEXT: usize = 10_000;

/// The platform, as herald names it.
pub(crate) const TEAMS: &str = "Microsoft Teams";

/// Why herald cannot check who may read where a Teams route posts.
pub const UNCHECKED: &str = "herald cannot ask Microsoft Teams who may read the channel or chat a route posts to; say vouch_unchecked = true on the route where only those meant to may read it";

/// Why a webhook's URL does not take a request.
pub(crate) fn unaddressed(webhook: &Secret) -> String {
    format!(
        "the secret {} holds no HTTPS URL; give it the URL the workflow's trigger, \"When a Teams webhook request is received\", shows",
        webhook.name()
    )
}

/// How a destination posts: through a Workflows webhook, its URL and the
/// URL's HTTPS origin, or as a bot.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Way {
    Webhook {
        webhook: Secret,
        origin: Option<String>,
    },
    Bot(Box<Bot>),
}

/// One Microsoft Teams destination, for one route. Its debug form names
/// its secrets alone, never the origin read from a webhook's URL.
#[derive(Clone, PartialEq, Eq)]
pub struct Teams {
    pub(crate) way: Way,
    waits: Waits,
}

impl fmt::Debug for Teams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut shown = f.debug_struct("Teams");
        match &self.way {
            Way::Webhook { webhook, .. } => shown.field("webhook", webhook),
            Way::Bot(bot) => shown.field("bot", bot),
        };
        shown.field("waits", &self.waits).finish_non_exhaustive()
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
            way: Way::Webhook { webhook, origin },
            waits: Waits::DEFAULT,
        }
    }

    /// The destination posting as `bot`.
    #[must_use]
    pub fn bot(bot: Bot) -> Self {
        Self {
            way: Way::Bot(Box::new(bot)),
            waits: Waits::DEFAULT,
        }
    }

    /// The destination, waiting `waits` where Teams names no wait.
    #[must_use]
    pub fn waiting(self, waits: Waits) -> Self {
        Self { waits, ..self }
    }

    /// What its `Http` may reach: the HTTPS origin of its webhook's URL
    /// alone, or nothing where the secret holds no such URL; a bot's
    /// sign-in service and service URL (ADR-0007).
    #[must_use]
    pub fn base(&self) -> Base<'_> {
        match &self.way {
            Way::Webhook { origin, .. } => origin.as_deref().map_or(Base::Among(&[]), Base::At),
            Way::Bot(bot) => Base::Among(bot.bases()),
        }
    }

    /// What it can do.
    #[must_use]
    pub fn profile(&self) -> Profile {
        match &self.way {
            Way::Webhook { .. } => profile(),
            Way::Bot(bot) => bot_profile(bot.threads()),
        }
    }

    /// Sends `step` of `announcement`, whose earlier steps `entry` holds,
    /// at `now`, and reads Teams' answer.
    ///
    /// # Errors
    ///
    /// When a request got no response at all.
    pub async fn send<H: Http>(
        &self,
        http: &H,
        (step, announcement, entry): (&Step, &Announcement, &Entry),
        now: Timestamp,
    ) -> Result<Sent, HttpError> {
        match &self.way {
            Way::Webhook { webhook, .. } => match request(webhook, step, announcement) {
                Ok(request) => Ok(read(&http.send(request).await?, self.waits)),
                Err(refusal) => Ok(Sent::Refused(refusal)),
            },
            Way::Bot(bot) => {
                bot.send(http, (step, announcement, entry), now, self.waits)
                    .await
            }
        }
    }
}

/// What Teams said in `response`: its error's code and message, where it
/// answered JSON that names them, or else its text, cut short.
pub(crate) fn said(response: &Response) -> String {
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

/// What a webhook's post answers: landed, with nothing more to record,
/// where Teams took it; a wait, or why Teams refused it, otherwise.
fn read(response: &Response, waits: Waits) -> Sent {
    response
        .failure(waits, None, TEAMS, || said(response))
        .unwrap_or_else(|| Sent::Landed(Facts::default()))
}

/// A Teams profile of `text` bytes a card and these `features`.
const fn profiled(text: usize, features: Features) -> Profile {
    Profile {
        markup: MARKUP,
        measure: Measure::Bytes,
        limits: Limits {
            title: TITLE,
            text,
            followup: text,
            thread_name: TITLE,
        },
        features,
        symbols: Symbols::DEFAULT,
    }
}

/// What a Microsoft Teams destination can do through a Workflows webhook:
/// post, and nothing more.
#[must_use]
pub const fn profile() -> Profile {
    profiled(
        TEXT,
        Features {
            threads: Threads::None,
            crosspost: false,
            pins: Pins::None,
            reads_back: false,
            edits: false,
            mentions: MentionStyle::None,
            interactions: false,
        },
    )
}

/// The bytes a bot's card keeps for the mentions of its announcement: room
/// beside its title, text, envelope and buttons within
/// [`crate::BOT_MESSAGE`] as UTF-16 counts it.
const MENTIONS_FIRST: usize = 3_000;

/// The bytes of mentions a card holding mentions alone takes: within
/// [`crate::BOT_MESSAGE`], beside its envelope, as UTF-16 counts it.
const MENTIONS_REST: usize = 30_000;

/// The bytes a mention writes beyond its member's id: its label in its text,
/// its entity's text and its entity's name, each at most
/// [`herald_core::Phrase::MENTIONED`] characters of four bytes and a number
/// of four digits, and its entity's JSON.
const MENTION_OVERHEAD: usize = 3 * (herald_core::Phrase::MENTIONED * 4 + 5 + 9) + 64;

/// How many mentions a bot's cards hold.
const MENTION_ROOM: MentionRoom = MentionRoom {
    first: MENTIONS_FIRST,
    rest: MENTIONS_REST,
    copies: 1,
    overhead: MENTION_OVERHEAD,
};

/// What a Microsoft Teams bot can do: post, edit and delete its posts,
/// mention the members subscribed, take commands, and, where its route says
/// `threads = true`, reply in their threads.
#[must_use]
pub const fn bot_profile(threads: bool) -> Profile {
    profiled(
        BOT_TEXT,
        Features {
            threads: if threads {
                Threads::Replies
            } else {
                Threads::None
            },
            crosspost: false,
            pins: Pins::None,
            reads_back: false,
            edits: true,
            mentions: MentionStyle::Users(MENTION_ROOM),
            interactions: true,
        },
    )
}
