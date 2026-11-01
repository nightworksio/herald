use herald_core::{
    Announcement, Chore, Delivery, Dialect, Entry, Features, Limits, Measure, MentionStyle, Pins,
    Profile, Sent, Step, Threads,
};
use herald_ports::{Http, HttpError, Request, Secret};

use crate::options::RouteOptions;
use crate::read::{read, trouble};
use crate::request::{Refusal, chore, step};
use crate::unpin::unpinned;

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
        }
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
        let bot = matches!(self.credential, Credential::Bot(_));
        Profile {
            dialect: Dialect::Discord,
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
                pins: match (bot, self.route.forum) {
                    (false, _) => Pins::None,
                    (true, false) => Pins::Many,
                    (true, true) => Pins::One,
                },
                reads_back: bot && !self.route.forum,
                edits: bot,
                mentions: MentionStyle::Roles,
                interactions: bot,
            },
        }
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
            if let Some(trouble) = trouble(&response, &body) {
                return Ok(trouble);
            }
        }
        Ok(Sent::Landed(herald_core::Facts::default()))
    }
}
