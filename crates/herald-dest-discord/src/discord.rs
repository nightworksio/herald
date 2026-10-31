use herald_core::{
    Announcement, Chore, Delivery, Dialect, Entry, Features, Limits, Measure, MentionStyle,
    Profile, Sent, Step, Threads,
};
use herald_ports::{Http, HttpError, Request, Secret};

use crate::options::RouteOptions;
use crate::read::{read, trouble};
use crate::request::{Refusal, chore, step};

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
                pins: bot,
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
        let request = match self.request(step_to_take, announcement, delivery, entry) {
            Ok(request) => request,
            Err(refusal) => return Ok(Sent::Refused(refusal.to_string())),
        };
        Ok(read(self, step_to_take, &http.send(request).await?))
    }

    /// Does `housekeeping` to the thread `thread`.
    ///
    /// # Errors
    ///
    /// When the request got no response at all.
    pub async fn housekeep<H: Http>(
        &self,
        http: &H,
        housekeeping: Chore,
        thread: &str,
    ) -> Result<Sent, HttpError> {
        let request = match chore(self, housekeeping, thread) {
            Ok(request) => request,
            Err(refusal) => return Ok(Sent::Refused(refusal.to_string())),
        };
        let response = http.send(request).await?;
        let body = serde_json::from_slice(&response.body).unwrap_or(serde_json::Value::Null);
        Ok(trouble(&response, &body).unwrap_or(Sent::Landed(herald_core::Facts::default())))
    }
}
