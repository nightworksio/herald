use herald_core::{
    Announcement, Bullet, Declined, Emphases, Entry, Escape, Facts, Features, Fences, Heading,
    Limits, Links, Marker, Markup, Measure, MentionRoom, MentionStyle, PinLookups, Pins, Profile,
    ReadBack, Sent, Step, Threads, Waits,
};
use herald_ports::{Http, HttpError, Learned, Request, Secret};
use serde_json::Value;

use crate::options::RouteOptions;
use crate::read::{UNNAMED, failure, json, read, text};
use crate::request::{
    MATRIX_TO, NOT_FOUND, ROUTE, Repin, event, get, pin_target, pinned, pins, repinned, step, with,
    without,
};

/// The platform, as herald names it.
pub(crate) const MATRIX: &str = "Matrix";

/// Matrix's HTML beside a plain body, its `matrix.to` pills never linked.
pub const MARKUP: Markup = Markup {
    escape: Escape::Neutral,
    links: Links::Spelled,
    heading: Heading::Bare,
    marks: Emphases {
        emphasis: "",
        strong: "",
        strike: "",
    },
    fences: Fences::None,
    bullet: Bullet::Symbol('-'),
    html: true,
    pills: Some(MATRIX_TO),
};

/// An answer read, a reason to stop with, or no answer at all.
pub(crate) type ApiAnswer<T> = Result<Result<T, Sent>, HttpError>;

/// The longest title and thread name, in bytes.
const TITLE: usize = 256;
/// The longest text of one message, in bytes. A message carries its text
/// twice, as its body and its HTML, within an event's 65,536 bytes.
const TEXT: usize = 16_384;

/// The most bytes an event holds.
const EVENT: usize = 65_536;
/// The bytes an announcement keeps beside its text and its mentions: its
/// title and link, each written twice, its markers and the event's own keys.
const KEPT: usize = 8_192;
/// The mentions an event holds. The announcement keeps what its text, written
/// twice, and `KEPT` leave; each message after it all but `KEPT`. A mention
/// writes its user id four times: in the body, twice in its pill, and in
/// `m.mentions`.
const MENTION_ROOM: MentionRoom = MentionRoom {
    first: EVENT - 2 * TEXT - KEPT,
    rest: EVENT - KEPT,
    copies: 4,
    overhead: 64,
};

/// One Matrix destination, for one route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    pub(crate) homeserver: String,
    pub(crate) token: Secret,
    pub(crate) route: RouteOptions,
    pub(crate) read_back: ReadBack,
    pub(crate) pin_lookups: PinLookups,
    pub(crate) waits: Waits,
    pub(crate) account: Learned<String>,
}

impl Matrix {
    /// The bot account whose access token is `token`, on the homeserver at
    /// `homeserver`, for a route whose Matrix options are `route`.
    #[must_use]
    pub fn new(homeserver: &str, token: Secret, route: RouteOptions) -> Self {
        Self {
            homeserver: homeserver.trim_end_matches('/').to_owned(),
            token,
            route,
            read_back: ReadBack::DEFAULT,
            pin_lookups: PinLookups::DEFAULT,
            waits: Waits::DEFAULT,
            account: Learned::new(),
        }
    }

    /// The destination, waiting `waits` where the homeserver names no
    /// wait.
    #[must_use]
    pub fn waiting(self, waits: Waits) -> Self {
        Self { waits, ..self }
    }

    /// The destination, looking up at most `pin_lookups` pins as it finds a
    /// route's earlier pins.
    #[must_use]
    pub fn looking_up(self, pin_lookups: PinLookups) -> Self {
        Self {
            pin_lookups,
            ..self
        }
    }

    /// The destination, reading back `read_back` pages of its own posts.
    #[must_use]
    pub fn reading_back(self, read_back: ReadBack) -> Self {
        Self { read_back, ..self }
    }

    /// The base URL of the homeserver it reaches, the only origin its
    /// `Http` may reach (ADR-0007).
    #[must_use]
    pub fn api(&self) -> &str {
        &self.homeserver
    }

    /// What the destination can do, as the core plans with it.
    #[must_use]
    pub fn profile(&self) -> Profile {
        profile()
    }

    /// The request for `step` of `announcement`, given what `entry` says
    /// already landed. A pin and an unpin are each read, then written, by
    /// [`Matrix::send`].
    ///
    /// # Errors
    ///
    /// When Matrix cannot take the step, or an earlier step it needs has not
    /// landed.
    pub fn request(
        &self,
        step_to_take: &Step,
        announcement: &Announcement,
        entry: &Entry,
    ) -> Result<Request, Declined> {
        step(self, step_to_take, announcement, entry)
    }

    /// Sends `step` of `announcement` and reads the homeserver's answer.
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
        if matches!(step_to_take, Step::Pin | Step::Unpin) {
            return self.repin(http, step_to_take, announcement, entry).await;
        }
        match self.request(step_to_take, announcement, entry) {
            Ok(request) => Ok(read(self, &http.send(request).await?)),
            Err(refusal) => Ok(Sent::Refused(refusal.to_string())),
        }
    }

    async fn repin<H: Http>(
        &self,
        http: &H,
        step_to_take: &Step,
        announcement: &Announcement,
        entry: &Entry,
    ) -> Result<Sent, HttpError> {
        let target = match pin_target(step_to_take, announcement, entry) {
            Ok(target) => target,
            Err(refusal) => return Ok(Sent::Refused(refusal.to_string())),
        };
        let held = match pins(&http.send(pinned(self)).await?, self.waits) {
            Ok(held) => held,
            Err(sent) => return Ok(sent),
        };
        let wanted = match target {
            Repin::Pin(post) => with(&held, post),
            Repin::Unpin {
                post,
                remembered: Some(earlier),
            } => without(&held, &[earlier.to_owned()], post),
            Repin::Unpin {
                post,
                remembered: None,
            } => match self
                .routed(http, &held, post, &announcement.marks.route)
                .await?
            {
                Ok(stale) => without(&held, &stale, post),
                Err(sent) => return Ok(sent),
            },
        };
        let landed = Sent::Landed(Facts::default());
        if wanted == held {
            return Ok(landed);
        }
        let answer = http.send(repinned(self, &wanted)).await?;
        Ok(failure(&answer, self.waits).unwrap_or(landed))
    }

    /// The pins in `held`, other than `post`, that herald's own account sent
    /// and that carry the route's marker `route`. A pin whose event is gone
    /// is not counted.
    async fn routed<H: Http>(
        &self,
        http: &H,
        held: &[String],
        post: &str,
        route: &Marker,
    ) -> ApiAnswer<Vec<String>> {
        let candidates: Vec<&String> = held.iter().filter(|pin| *pin != post).collect();
        if let Some(over) = self.pin_lookups.over(candidates.len()) {
            return Ok(Err(Sent::Refused(over)));
        }
        let herald = match self.account(http).await? {
            Ok(herald) => herald,
            Err(sent) => return Ok(Err(sent)),
        };
        let mut stale = Vec::new();
        for pin in candidates {
            let response = http.send(event(self, pin)).await?;
            if response.status == NOT_FOUND {
                continue;
            }
            if let Some(sent) = failure(&response, self.waits) {
                return Ok(Err(sent));
            }
            let found = json(&response);
            let content = found.get("content").unwrap_or(&Value::Null);
            if text(&found, "sender") == Some(herald.as_str())
                && text(content, ROUTE) == Some(route.as_str())
            {
                stale.push(pin.clone());
            }
        }
        Ok(Ok(stale))
    }

    /// The account the token is, as the homeserver names it, asked once.
    pub(crate) async fn account<H: Http>(&self, http: &H) -> ApiAnswer<String> {
        if let Some(account) = self.account.get() {
            return Ok(Ok(account.clone()));
        }
        let answer = http.send(get(self, "/account/whoami")).await?;
        if let Some(sent) = failure(&answer, self.waits) {
            return Ok(Err(sent));
        }
        Ok(text(&json(&answer), "user_id")
            .map(|account| self.account.learn(account.to_owned()).clone())
            .ok_or_else(|| Sent::Refused(UNNAMED.to_owned())))
    }
}

/// What a Matrix destination can do.
#[must_use]
pub fn profile() -> Profile {
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
            threads: Threads::Replies,
            crosspost: false,
            pins: Pins::Many,
            reads_back: true,
            edits: true,
            mentions: MentionStyle::Users(MENTION_ROOM),
            interactions: false,
        },
        symbols: herald_core::Symbols::DEFAULT,
    }
}
