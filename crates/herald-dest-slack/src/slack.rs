use herald_core::{
    Announcement, Dialect, Entry, Facts, Features, Limits, Marker, Measure, MentionRoom,
    MentionStyle, Pins, Profile, ReadBack, Sent, Step, Threads, Wording,
};
use herald_ports::{Http, HttpError, Request, Secret, unreserved};
use serde_json::Value;

use crate::domain::Domain;
use crate::options::RouteOptions;
use crate::read::{json, read, text, trouble};
use crate::request::{EVENT, ROUTE, Refusal, channel, get, pin, posted, step};
use crate::web_api::{
    AUTH_TEST, BOTS_INFO, CHAT_GET_PERMALINK, CONVERSATIONS_HISTORY, PINS_REMOVE,
};

/// The longest title, in characters: a header block's text.
const TITLE: usize = 150;
/// The longest text of one message, in characters: a section block's text.
const TEXT: usize = 3_000;
/// What Slack answers for a pin already in place.
const ALREADY_PINNED: &str = "already_pinned";
/// What Slack answers for an unpin of a message no longer pinned, or gone.
const UNPINNED: [&str; 2] = ["no_pin", "message_not_found"];

/// An answer read, a reason to stop with, or no answer at all.
pub(crate) type Read<T> = Result<Result<T, Sent>, HttpError>;

/// How herald is allowed to post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    /// A bot's token.
    Bot(Secret),
    /// An incoming webhook's URL, which carries its own token.
    Webhook(Secret),
}

/// The identity herald posts as (ADR-0011): its bot, and the app it
/// belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Identity {
    pub(crate) bot: String,
    pub(crate) app: String,
}

impl Identity {
    /// Whether `message` was posted by this identity.
    pub(crate) fn posted(&self, message: &Value) -> bool {
        text(message, "bot_id") == Some(self.bot.as_str())
            && text(message, "app_id") == Some(self.app.as_str())
    }
}

/// The base URL a destination reaches: `given`, or `domain`'s Web API for a
/// bot and its webhooks for a webhook.
fn base(given: Option<&str>, domain: Domain, credential: &Credential) -> String {
    given.map_or_else(
        || match credential {
            Credential::Bot(_) => domain.api(),
            Credential::Webhook(_) => domain.hooks(),
        },
        ToOwned::to_owned,
    )
}

/// One Slack destination, for one route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slack {
    pub(crate) api: String,
    given: Option<String>,
    pub(crate) credential: Credential,
    pub(crate) route: RouteOptions,
    pub(crate) read_back: ReadBack,
    pub(crate) team: Option<String>,
    pub(crate) wording: Wording,
}

impl Slack {
    /// The destination reached with `credential`, for a route whose Slack
    /// options are `route`, at `api` or Slack's own: its Web API for a bot,
    /// its webhooks for a webhook.
    #[must_use]
    pub fn new(credential: Credential, route: RouteOptions, api: Option<&str>) -> Self {
        let given = api.map(|api| api.trim_end_matches('/').to_owned());
        Self {
            api: base(given.as_deref(), Domain::default(), &credential),
            given,
            credential,
            route,
            read_back: ReadBack::DEFAULT,
            team: None,
            wording: Wording::default(),
        }
    }

    /// The destination, its subscribe button labelled as `wording` words
    /// it.
    #[must_use]
    pub fn worded(self, wording: Wording) -> Self {
        Self { wording, ..self }
    }

    /// The destination, as an Enterprise Grid org-wide app installed in the
    /// workspace `team`, which each Web API call names.
    #[must_use]
    pub fn in_team(self, team: Option<&str>) -> Self {
        Self {
            team: team.map(ToOwned::to_owned),
            ..self
        }
    }

    /// The destination, on the Slack `domain` names, where no `api` was given.
    #[must_use]
    pub fn on(self, domain: Domain) -> Self {
        Self {
            api: base(self.given.as_deref(), domain, &self.credential),
            ..self
        }
    }

    /// The destination, reading back `read_back` pages of its own posts.
    #[must_use]
    pub fn reading_back(self, read_back: ReadBack) -> Self {
        Self { read_back, ..self }
    }

    /// The base URL it reaches, the only origin its `Http` may reach
    /// (ADR-0007).
    #[must_use]
    pub fn api(&self) -> &str {
        &self.api
    }

    /// What the destination can do, as the core plans with it.
    #[must_use]
    pub fn profile(&self) -> Profile {
        profile(matches!(self.credential, Credential::Bot(_)))
    }

    /// The request for `step` of `announcement`, given what `entry` says
    /// already landed. An unpin is read, then written, by [`Slack::send`].
    ///
    /// # Errors
    ///
    /// When Slack cannot take the step, or an earlier step it needs has not
    /// landed.
    pub fn request(
        &self,
        step_to_take: &Step,
        announcement: &Announcement,
        entry: &Entry,
    ) -> Result<Request, Refusal> {
        step(self, step_to_take, announcement, entry)
    }

    /// Sends `step` of `announcement` and reads Slack's answer.
    ///
    /// # Errors
    ///
    /// When a request got no response at all.
    pub async fn send<H: Http>(
        &self,
        http: &H,
        step_to_take: &Step,
        announcement: &Announcement,
        entry: &Entry,
    ) -> Result<Sent, HttpError> {
        let request = match self.request(step_to_take, announcement, entry) {
            Ok(request) => request,
            Err(refusal) => return Ok(Sent::Refused(refusal.to_string())),
        };
        match step_to_take {
            Step::Unpin => self.unpin(http, request, announcement, entry).await,
            Step::Pin => {
                let answer = http.send(request).await?;
                let body = json(&answer);
                if text(&body, "error") == Some(ALREADY_PINNED) {
                    return Ok(Sent::Landed(Facts::default()));
                }
                Ok(trouble(&answer, &body).unwrap_or(Sent::Landed(Facts::default())))
            }
            Step::Post => match read(&http.send(request).await?) {
                Sent::Landed(facts) => Ok(Sent::Landed(self.linked(http, facts).await?)),
                sent => Ok(sent),
            },
            _ => Ok(read(&http.send(request).await?)),
        }
    }

    /// `facts` with the message's permalink, where Slack gives one. A post
    /// whose permalink cannot be read has still landed.
    async fn linked<H: Http>(&self, http: &H, facts: Facts) -> Result<Facts, HttpError> {
        let (Some(ts), Ok(channel)) = (facts.message.as_deref(), channel(self)) else {
            return Ok(facts);
        };
        let asked = format!(
            "channel={}&message_ts={}",
            unreserved(channel),
            unreserved(ts)
        );
        let answer = http.send(get(self, CHAT_GET_PERMALINK, &asked)).await?;
        let body = json(&answer);
        let permalink = trouble(&answer, &body)
            .is_none()
            .then(|| text(&body, "permalink").map(ToOwned::to_owned))
            .flatten();
        Ok(Facts { permalink, ..facts })
    }

    /// Unpins the route's earlier pins: the one the store remembers, or else
    /// each of herald's own pins that carries the route's marker. The
    /// announcement's own pin stays (ADR-0011).
    async fn unpin<H: Http>(
        &self,
        http: &H,
        listing: Request,
        announcement: &Announcement,
        entry: &Entry,
    ) -> Result<Sent, HttpError> {
        let (channel, post) = match (
            channel(self),
            posted(entry, &Step::Post, "the announcement's message"),
        ) {
            (Ok(channel), Ok(post)) => (channel, post),
            (Err(refusal), _) | (_, Err(refusal)) => return Ok(Sent::Refused(refusal.to_string())),
        };
        let stale = match &announcement.replaces {
            Some(remembered) => vec![remembered.clone()],
            None => match self
                .routed(http, listing, channel, post, &announcement.marks.route)
                .await?
            {
                Ok(stale) => stale,
                Err(sent) => return Ok(sent),
            },
        };
        for ts in stale.iter().filter(|ts| *ts != post) {
            let answer = http.send(pin(self, PINS_REMOVE, channel, ts)).await?;
            let body = json(&answer);
            if text(&body, "error").is_some_and(|error| UNPINNED.contains(&error)) {
                continue;
            }
            if let Some(sent) = trouble(&answer, &body) {
                return Ok(sent);
            }
        }
        Ok(Sent::Landed(Facts::default()))
    }

    /// The pinned messages of `channel` other than `post`, listed by
    /// `listing`, that herald's own identity posted and whose metadata
    /// carries the route's marker `route`.
    async fn routed<H: Http>(
        &self,
        http: &H,
        listing: Request,
        channel: &str,
        post: &str,
        route: &Marker,
    ) -> Read<Vec<String>> {
        let answer = http.send(listing).await?;
        let listed = json(&answer);
        if let Some(sent) = trouble(&answer, &listed) {
            return Ok(Err(sent));
        }
        let identity = match identity(self, http).await? {
            Ok(identity) => identity,
            Err(sent) => return Ok(Err(sent)),
        };
        let own: Vec<&str> = listed
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("message"))
            .filter(|message| identity.posted(message))
            .filter_map(|message| text(message, "ts"))
            .filter(|ts| *ts != post)
            .collect();
        let mut stale = Vec::new();
        for ts in own {
            let asked = format!(
                "channel={}&latest={ts}&oldest={ts}&inclusive=true&limit=1&include_all_metadata=true",
                unreserved(channel),
                ts = unreserved(ts)
            );
            let answer = http.send(get(self, CONVERSATIONS_HISTORY, &asked)).await?;
            let found = json(&answer);
            if let Some(sent) = trouble(&answer, &found) {
                return Ok(Err(sent));
            }
            let routed = found
                .get("messages")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|message| {
                    identity.posted(message) && carried(message, ROUTE) == Some(route.as_str())
                });
            if routed {
                stale.push(ts.to_owned());
            }
        }
        Ok(Ok(stale))
    }
}

/// The marker `field` of a message's herald metadata.
pub(crate) fn carried<'a>(message: &'a Value, field: &str) -> Option<&'a str> {
    let metadata = message.get("metadata")?;
    (text(metadata, "event_type") == Some(EVENT))
        .then(|| {
            metadata
                .get("event_payload")
                .and_then(|payload| text(payload, field))
        })
        .flatten()
}

/// Who the bot token is: its bot, from `auth.test`, and that bot's app,
/// from `bots.info`.
pub(crate) async fn identity<H: Http>(slack: &Slack, http: &H) -> Read<Identity> {
    let answer = http.send(get(slack, AUTH_TEST, "")).await?;
    let tested = json(&answer);
    if let Some(sent) = trouble(&answer, &tested) {
        return Ok(Err(sent));
    }
    let Some(bot) = text(&tested, "bot_id") else {
        return Ok(Err(Sent::Refused(
            "Slack did not say which bot the token is; give a bot token (xoxb-…)".to_owned(),
        )));
    };
    let answer = http
        .send(get(slack, BOTS_INFO, &format!("bot={}", unreserved(bot))))
        .await?;
    let info = json(&answer);
    if let Some(sent) = trouble(&answer, &info) {
        return Ok(Err(sent));
    }
    let Some(app) = info.get("bot").and_then(|bot| text(bot, "app_id")) else {
        return Ok(Err(Sent::Refused(
            "Slack did not say which app the bot belongs to".to_owned(),
        )));
    };
    Ok(Ok(Identity {
        bot: bot.to_owned(),
        app: app.to_owned(),
    }))
}

/// The most characters a section's text holds.
const SECTION: usize = 3_000;
/// The mentions a message holds: one section of them, each `<@USER> `.
const MENTION_ROOM: MentionRoom = MentionRoom {
    first: SECTION,
    rest: SECTION,
    copies: 1,
    overhead: 4,
};

/// What a Slack destination can do: as a bot, or through a webhook.
#[must_use]
pub fn profile(bot: bool) -> Profile {
    Profile {
        dialect: Dialect::Slack,
        measure: Measure::Chars,
        limits: Limits {
            title: TITLE,
            text: TEXT,
            followup: TEXT,
            thread_name: TITLE,
        },
        features: Features {
            threads: if bot { Threads::Replies } else { Threads::None },
            crosspost: false,
            pins: if bot { Pins::Many } else { Pins::None },
            reads_back: bot,
            edits: bot,
            mentions: MentionStyle::Users(MENTION_ROOM),
            interactions: bot,
        },
        symbols: herald_core::Symbols::DEFAULT,
    }
}
