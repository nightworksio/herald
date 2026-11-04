use herald_core::{
    ACTIVITY_JSON, Addressed, Announcement, Dialect, Entry, Facts, Features, Limits, Measure,
    MentionStyle, Pins, Profile, Sent, Step, Threads, signature_header, signing,
};
use herald_ports::{
    Algorithm, Http, HttpError, Method, PUBLIC, Request, Response, Secret, Signer, Text, USER_AGENT,
};
use jiff::{SignedDuration, Timestamp};
use serde_json::Value;

use super::documents::{accept, actor_id, create, key_id, note_id};
use super::options::ActorRouteOptions;

/// The longest title, in characters.
const TITLE: usize = 500;
/// The longest text of one Note, in characters. A Note holds the whole
/// announcement, so its parts are never posted apart.
const TEXT: usize = 20_000;
/// How long to wait when an inbox is unwell and says nothing of when to ask
/// again.
const OUTAGE: SignedDuration = SignedDuration::from_secs(5);
const TOO_MANY: u16 = 429;
const SERVER: u16 = 500;
/// What an inbox answers when its actor or server is gone for good.
const GONE: u16 = 410;
/// An HTTP date, as `Date` carries it.
const HTTP_DATE: &str = "%a, %d %b %Y %H:%M:%S GMT";

/// herald's own actor, announcing for one route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityPub {
    domain: String,
    key: Secret,
    route: ActorRouteOptions,
}

fn refused(reason: impl Into<String>) -> Sent {
    Sent::Refused(reason.into())
}

fn cannot(what: &str) -> Sent {
    refused(format!("herald's ActivityPub actor cannot {what}"))
}

impl ActivityPub {
    /// The actor `route` names on `domain`, signing with `key`.
    #[must_use]
    pub fn new(domain: &str, key: Secret, route: ActorRouteOptions) -> Self {
        Self {
            domain: domain.to_owned(),
            key,
            route,
        }
    }

    /// [`PUBLIC`]: its deliveries reach its followers' inboxes, on any public
    /// address (ADR-0007).
    #[must_use]
    pub fn api(&self) -> &'static str {
        PUBLIC
    }

    /// The actor's name, whose followers are its recipients.
    #[must_use]
    pub fn actor(&self) -> &str {
        &self.route.actor
    }

    /// The actor's id.
    #[must_use]
    pub fn id(&self) -> String {
        actor_id(&self.domain, &self.route.actor)
    }

    /// What the destination can do, as the core plans with it.
    #[must_use]
    pub fn profile(&self) -> Profile {
        actor_profile()
    }

    /// Takes `step` of `announcement`. The post is the Note itself, which
    /// herald serves and so needs no request, and a part is carried in it.
    /// A delivery is the Note's `Create`, signed at `now` through `signer`,
    /// posted to one inbox.
    ///
    /// # Errors
    ///
    /// When a delivery got no response at all.
    pub async fn send<H: Http, G: Signer>(
        &self,
        http: &H,
        signer: &G,
        now: Timestamp,
        step: &Step,
        announcement: &Announcement,
        _: &Entry,
    ) -> Result<Sent, HttpError> {
        match step {
            Step::Post => {
                let note = note_id(
                    &self.domain,
                    &self.route.actor,
                    &announcement.key.to_string(),
                );
                Ok(Sent::Landed(Facts {
                    message: Some(note.clone()),
                    thread: None,
                    permalink: Some(note),
                }))
            }
            Step::Chunk(_) => Ok(Sent::Landed(Facts::default())),
            Step::Deliver(inbox) => self.deliver(http, signer, now, inbox, announcement).await,
            Step::Crosspost => Ok(cannot("crosspost")),
            Step::OpenThread => Ok(cannot("open a thread")),
            Step::Pin | Step::Unpin => Ok(cannot("pin")),
        }
    }

    async fn deliver<H: Http, G: Signer>(
        &self,
        http: &H,
        signer: &G,
        now: Timestamp,
        inbox: &str,
        announcement: &Announcement,
    ) -> Result<Sent, HttpError> {
        let created = create(
            &self.domain,
            &self.route.actor,
            announcement,
            self.route.content_warning.as_deref(),
        );
        self.post(http, signer, now, inbox, &created).await
    }

    /// The `Accept` of `follow`, a `Follow` of this actor.
    #[must_use]
    pub fn accept(&self, follow: &Value) -> Value {
        accept(&self.domain, &self.route.actor, follow)
    }

    /// Posts `activity` to `inbox`, signed by this actor at `now` through
    /// `signer`, and reads the inbox's answer.
    ///
    /// # Errors
    ///
    /// When the inbox gave no response at all.
    pub async fn post<H: Http, G: Signer>(
        &self,
        http: &H,
        signer: &G,
        now: Timestamp,
        inbox: &str,
        activity: &Value,
    ) -> Result<Sent, HttpError> {
        let Some((host, target)) = addressed(inbox) else {
            return Ok(refused(format!(
                "\"{inbox}\" is not an https inbox, so nothing was delivered to it"
            )));
        };
        let body = activity.to_string();
        let date = now.strftime(HTTP_DATE).to_string();
        let to = Addressed {
            target: &target,
            audience: &host,
        };
        let delivered = signing(to, &date, body.as_bytes());
        let signature = match signer
            .sign(Algorithm::RsaSha256, &self.key, delivered.string.as_bytes())
            .await
        {
            Ok(signature) => signature,
            Err(error) => return Ok(refused(error.to_string())),
        };
        let header = signature_header(&key_id(&self.domain, &self.route.actor), &signature);
        let request = Request::new(Method::Post, Text::plain(inbox))
            .header("User-Agent", Text::plain(USER_AGENT))
            .header("Content-Type", Text::plain(ACTIVITY_JSON))
            .header("Date", Text::plain(date))
            .header("Digest", Text::plain(delivered.digest))
            .header("Signature", Text::plain(header))
            .body(body);
        Ok(answered(&http.send(request).await?))
    }
}

/// The host and the request target an inbox's signature names, where the
/// inbox is an https URL.
fn addressed(inbox: &str) -> Option<(String, String)> {
    let url = url::Url::parse(inbox).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    let host = match url.port() {
        Some(port) => format!("{}:{port}", url.host_str()?),
        None => url.host_str()?.to_owned(),
    };
    let target = match url.query() {
        Some(query) => format!("post {}?{query}", url.path()),
        None => format!("post {}", url.path()),
    };
    Some((host, target))
}

/// What an inbox's answer means. An inbox that is gone for good counts as
/// delivered, as nothing will ever take it there.
fn answered(response: &Response) -> Sent {
    if response.succeeded() || response.status == GONE {
        return Sent::Landed(Facts::default());
    }
    if response.status == TOO_MANY || response.status >= SERVER {
        let after = response
            .header("retry-after")
            .and_then(|after| after.parse::<u32>().ok())
            .map(|seconds| SignedDuration::from_secs(i64::from(seconds)));
        return Sent::RetryAfter(after.unwrap_or(OUTAGE));
    }
    refused(format!("the inbox refused it with {}", response.status))
}

/// What herald's ActivityPub actor can do: one Note per announcement,
/// delivered to its followers, and nothing it can read back, pin or edit.
#[must_use]
pub fn actor_profile() -> Profile {
    Profile {
        dialect: Dialect::Matrix,
        measure: Measure::Chars,
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
    }
}
