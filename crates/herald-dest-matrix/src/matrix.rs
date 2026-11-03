use herald_core::{
    Announcement, Dialect, Entry, Facts, Features, Limits, Marker, Measure, MentionStyle, Pins,
    Profile, Sent, Step, Threads,
};
use herald_ports::{Http, HttpError, Request, Secret};
use serde_json::Value;

use crate::options::RouteOptions;
use crate::read::{UNNAMED, json, read, text, trouble};
use crate::request::{
    NOT_FOUND, ROUTE, Refusal, Repin, event, get, pin_target, pinned, pins, repinned, step, with,
    without,
};

/// An answer read, a reason to stop with, or no answer at all.
type Read<T> = Result<Result<T, Sent>, HttpError>;

/// The longest title and thread name, in bytes.
const TITLE: usize = 256;
/// The longest text of one message, in bytes. A message carries its text
/// twice, as its body and its HTML, within an event's 65,536 bytes.
const TEXT: usize = 16_384;

/// One Matrix destination, for one route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    pub(crate) homeserver: String,
    pub(crate) token: Secret,
    pub(crate) route: RouteOptions,
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
        }
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
    ) -> Result<Request, Refusal> {
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
        let held = match pins(&http.send(pinned(self)).await?) {
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
        Ok(trouble(&answer, &json(&answer)).unwrap_or(landed))
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
    ) -> Read<Vec<String>> {
        let account = http.send(get(self, "/account/whoami")).await?;
        let named = json(&account);
        if let Some(sent) = trouble(&account, &named) {
            return Ok(Err(sent));
        }
        let Some(herald) = text(&named, "user_id") else {
            return Ok(Err(Sent::Refused(UNNAMED.to_owned())));
        };
        let mut stale = Vec::new();
        for pin in held.iter().filter(|pin| *pin != post) {
            let response = http.send(event(self, pin)).await?;
            if response.status == NOT_FOUND {
                continue;
            }
            let found = json(&response);
            if let Some(sent) = trouble(&response, &found) {
                return Ok(Err(sent));
            }
            let content = found.get("content").unwrap_or(&Value::Null);
            if text(&found, "sender") == Some(herald)
                && text(content, ROUTE) == Some(route.as_str())
            {
                stale.push(pin.clone());
            }
        }
        Ok(Ok(stale))
    }
}

/// What a Matrix destination can do.
#[must_use]
pub fn profile() -> Profile {
    Profile {
        dialect: Dialect::Matrix,
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
            mentions: MentionStyle::Users,
            interactions: false,
        },
    }
}
