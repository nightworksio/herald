use herald_core::{
    Announcement, Dialect, Entry, Features, Limits, Measure, MentionStyle, Pins, Profile, Sent,
    Step, Threads,
};
use herald_ports::{Http, HttpError, Request, Secret};

use crate::options::RouteOptions;
use crate::read::{read, trouble};
use crate::request::{Refusal, pin_target, pinned, pinning, step};

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
        let (post, earlier) = match pin_target(step_to_take, announcement, entry) {
            Ok(target) => target,
            Err(refusal) => return Ok(Sent::Refused(refusal.to_string())),
        };
        let response = http.send(pinned(self)).await?;
        let (held, landed) = pinning(self, post, earlier, &response);
        let Some(request) = held else {
            return Ok(landed);
        };
        let answer = http.send(request).await?;
        let body = serde_json::from_slice(&answer.body).unwrap_or(serde_json::Value::Null);
        Ok(trouble(&answer, &body).unwrap_or(landed))
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
