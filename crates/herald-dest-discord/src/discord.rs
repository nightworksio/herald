use herald_core::{
    Announcement, Bullet, Chore, Delivery, Emphases, Entry, Escape, Features, Fences, Heading,
    Limits, Links, Markup, Measure, MentionStyle, Pins, Profile, ReadBack, Sent, Step, Threads,
    Waits, Wording,
};
use herald_ports::{Http, HttpError, Request, Secret};

use crate::look::Look;
use crate::options::RouteOptions;
use crate::read::{read, trouble};
use crate::request::{Refusal, chore, step};
use crate::unpin::unpinned;

/// Discord's Markdown: punctuation escaped, links bracketed, headings hashed.
pub const MARKUP: Markup = Markup {
    escape: Escape::Backslash,
    links: Links::Bracketed,
    heading: Heading::Hashes(3),
    marks: Emphases {
        emphasis: "*",
        strong: "**",
        strike: "~~",
    },
    fences: Fences::Labelled,
    bullet: Bullet::Fixed("- "),
    html: false,
    pills: None,
};

/// The API herald speaks, at the version it was written against.
pub(crate) const API: &str = "https://discord.com/api/v10";

/// How herald is allowed to post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    /// A bot's token.
    Bot(Secret),
    /// An incoming webhook's URL, which carries its own token.
    Webhook(Secret),
}

/// One Discord destination, for one route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discord {
    pub(crate) api: String,
    pub(crate) credential: Credential,
    pub(crate) route: RouteOptions,
    pub(crate) read_back: ReadBack,
    pub(crate) look: Look,
    pub(crate) wording: Wording,
    pub(crate) waits: Waits,
}

impl Discord {
    /// The destination reached with `credential`, for a route whose Discord
    /// options are `route`, at `api` or Discord's own API.
    #[must_use]
    pub fn new(credential: Credential, route: RouteOptions, api: Option<&str>) -> Self {
        Self {
            api: api.unwrap_or(API).trim_end_matches('/').to_owned(),
            credential,
            route,
            read_back: ReadBack::DEFAULT,
            look: Look::DEFAULT,
            wording: Wording::default(),
            waits: Waits::DEFAULT,
        }
    }

    /// The destination, waiting `waits` where Discord names no wait.
    #[must_use]
    pub fn waiting(self, waits: Waits) -> Self {
        Self { waits, ..self }
    }

    /// The destination, reading back `read_back` pages of its own posts.
    #[must_use]
    pub fn reading_back(self, read_back: ReadBack) -> Self {
        Self { read_back, ..self }
    }

    /// The destination, its posts' buttons labelled as `wording` words
    /// them.
    #[must_use]
    pub fn worded(self, wording: Wording) -> Self {
        Self { wording, ..self }
    }

    /// The destination, its posts looking as `look` says.
    #[must_use]
    pub fn looking(self, look: Look) -> Self {
        Self { look, ..self }
    }

    /// The base URL of the API it reaches, the only origin its `Http` may
    /// reach (ADR-0007).
    #[must_use]
    pub fn api(&self) -> &str {
        &self.api
    }

    /// What the destination can do, as the core plans with it.
    #[must_use]
    pub fn profile(&self) -> Profile {
        profile(
            matches!(self.credential, Credential::Bot(_)),
            self.route.forum,
        )
    }

    /// The request for `step` of `announcement`, given what `entry` says
    /// already landed.
    ///
    /// # Errors
    ///
    /// When this destination cannot take the step, or an earlier step it
    /// needs has not landed.
    pub fn request(
        &self,
        step_to_take: &Step,
        announcement: &Announcement,
        delivery: &Delivery,
        entry: &Entry,
    ) -> Result<Request, Refusal> {
        step(self, step_to_take, announcement, delivery, entry)
    }

    /// Sends `step` of `announcement` and reads Discord's answer.
    ///
    /// # Errors
    ///
    /// When the request got no response at all.
    pub async fn send<H: Http>(
        &self,
        http: &H,
        step_to_take: &Step,
        announcement: &Announcement,
        delivery: &Delivery,
        entry: &Entry,
    ) -> Result<Sent, HttpError> {
        if *step_to_take == Step::Unpin {
            return unpinned(self, http, announcement, entry).await;
        }
        let request = match self.request(step_to_take, announcement, delivery, entry) {
            Ok(request) => request,
            Err(refusal) => return Ok(Sent::Refused(refusal.to_string())),
        };
        Ok(read(self, step_to_take, &http.send(request).await?))
    }

    /// Does `housekeeping` to the thread `thread`. A lock unarchives the
    /// thread to lock it, as Discord changes no archived thread otherwise, and
    /// archives it again.
    ///
    /// # Errors
    ///
    /// When a request got no response at all.
    pub async fn housekeep<H: Http>(
        &self,
        http: &H,
        housekeeping: Chore,
        thread: &str,
    ) -> Result<Sent, HttpError> {
        let requests = match chore(self, housekeeping, thread) {
            Ok(requests) => requests,
            Err(refusal) => return Ok(Sent::Refused(refusal.to_string())),
        };
        for request in requests {
            let response = http.send(request).await?;
            let body = serde_json::from_slice(&response.body).unwrap_or(serde_json::Value::Null);
            if let Some(trouble) = trouble(&response, &body, self.waits) {
                return Ok(trouble);
            }
        }
        Ok(Sent::Landed(herald_core::Facts::default()))
    }
}

/// What a Discord destination can do: as a bot or through a webhook, into a
/// forum or a channel.
#[must_use]
pub fn profile(bot: bool, forum: bool) -> Profile {
    Profile {
        markup: MARKUP,
        measure: Measure::Chars,
        limits: Limits {
            title: 256,
            text: 4096,
            followup: 2000,
            thread_name: 100,
        },
        features: Features {
            threads: if bot {
                Threads::FromMessage
            } else {
                Threads::None
            },
            crosspost: bot,
            pins: match (bot, forum) {
                (false, _) => Pins::None,
                (true, false) => Pins::Many,
                (true, true) => Pins::One,
            },
            reads_back: bot && !forum,
            edits: bot,
            mentions: MentionStyle::Roles,
            interactions: bot,
        },
        symbols: herald_core::Symbols::DEFAULT,
    }
}
