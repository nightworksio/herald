use herald_core::{
    ACTIVITY_JSON, Addressed, Announcement, Bullet, Declined, Emphases, Entry, Escape, Facts,
    Features, Fences, Heading, Limits, Links, Markup, Measure, MentionStyle, Pins, Profile, Sent,
    Step, Threads, Waits, signature_header, signing,
};
use herald_ports::{
    Algorithm, CryptoError, Http, HttpError, Method, PUBLIC, Request, Response, Secret, Signer,
    Text, USER_AGENT,
};
use jiff::Timestamp;
use serde_json::Value;

use super::documents::{
    accept, actor_document, actor_id, create, follow, followers_collection, key_id, note, note_id,
    outbox, outbox_page, reject, webfinger,
};
use super::options::{ActorOptions, ActorRouteOptions};
use super::policy::Policy;

/// The HTML of a Note, beside its plain text.
pub const ACTOR_MARKUP: Markup = Markup {
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
    pills: None,
};

/// The longest title, in characters.
const TITLE: usize = 500;
/// The longest text of one Note, in characters. A Note holds the whole
/// announcement, so its parts are never posted apart.
const TEXT: usize = 20_000;
/// What an inbox answers when its actor or server is gone for good.
const GONE: u16 = 410;
/// The most of an inbox's answer read: only its status is used.
const ANSWER: usize = 65_536;
/// An HTTP date, as `Date` carries it.
const HTTP_DATE: &str = "%a, %d %b %Y %H:%M:%S GMT";

/// herald's own actor, announcing for one route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityPub {
    domain: String,
    origin: String,
    key: Secret,
    route: ActorRouteOptions,
    waits: Waits,
    page: usize,
    deliveries: usize,
    policy: Policy,
}

fn refused(reason: impl Into<String>) -> Sent {
    Sent::Refused(reason.into())
}

/// The destination, as herald names it.
pub(crate) const ACTOR: &str = "herald's ActivityPub actor";

fn cannot(what: &'static str) -> Sent {
    refused(
        Declined::Cannot {
            platform: ACTOR,
            what,
        }
        .to_string(),
    )
}

impl ActivityPub {
    /// The actor `route` names on `domain`, served under the path `base`,
    /// signing with `key`.
    #[must_use]
    pub fn new(domain: &str, base: &str, key: Secret, route: ActorRouteOptions) -> Self {
        Self {
            domain: domain.to_owned(),
            origin: format!("{domain}{base}"),
            key,
            route,
            waits: Waits::DEFAULT,
            page: ActorOptions::OUTBOX_PAGE,
            deliveries: ActorOptions::DELIVERIES,
            policy: Policy::default(),
        }
    }

    /// The actor, federating as `policy` says.
    #[must_use]
    pub fn governed(self, policy: Policy) -> Self {
        Self { policy, ..self }
    }

    /// Whom the actor federates with, and how many may follow it.
    #[must_use]
    pub const fn policy(&self) -> &Policy {
        &self.policy
    }

    /// The actor, waiting `waits` where an inbox names no wait.
    #[must_use]
    pub fn waiting(self, waits: Waits) -> Self {
        Self { waits, ..self }
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
        actor_id(&self.origin, &self.route.actor)
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
                    &self.origin,
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
            Step::Deliveries => Ok(cannot("deliver to every inbox in one request")),
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
        let created = create(&self.origin, announcement, &self.route);
        self.post(http, signer, now, inbox, &created).await
    }

    /// The domain the actor is served on.
    #[must_use]
    pub fn domain(&self) -> &str {
        &self.domain
    }

    /// The actor's document, as herald serves it at its id, its public key
    /// read from its private key through `signer`.
    ///
    /// # Errors
    ///
    /// When `signer` cannot read the public key.
    pub async fn document<G: Signer>(&self, signer: &G) -> Result<String, CryptoError> {
        let public = signer.public(Algorithm::RsaSha256, &self.key).await?;
        Ok(actor_document(
            &self.origin,
            &self.route.actor,
            &public,
            self.policy.manual(),
        )
        .to_string())
    }

    /// What WebFinger answers for the actor.
    #[must_use]
    pub fn finger(&self) -> String {
        webfinger(&self.domain, &self.origin, &self.route.actor).to_string()
    }

    /// `announcement`'s Note, as herald serves it at its id.
    #[must_use]
    pub fn note(&self, announcement: &Announcement) -> String {
        note(&self.origin, announcement, &self.route).to_string()
    }

    /// The actor's outbox, holding `total` Notes.
    #[must_use]
    pub fn outbox(&self, total: usize) -> String {
        outbox(&self.origin, &self.route.actor, total).to_string()
    }

    /// The page `page` of the actor's outbox: those of `notes` that are the
    /// actor's own, newest first, and the page after it where `next` names
    /// one.
    #[must_use]
    pub fn outbox_page(&self, page: &str, notes: &[String], next: Option<&str>) -> String {
        let id = self.id();
        let own: Vec<Value> = notes
            .iter()
            .filter_map(|note| serde_json::from_str::<Value>(note).ok())
            .filter(|note| note.get("attributedTo").and_then(Value::as_str) == Some(id.as_str()))
            .collect();
        outbox_page(&self.origin, &self.route.actor, page, &own, next).to_string()
    }

    /// The actor's followers collection, holding how many of `total` follow
    /// it and never who.
    #[must_use]
    pub fn followers(&self, total: usize) -> String {
        followers_collection(&self.origin, &self.route.actor, total).to_string()
    }

    /// How many Notes one page of its outbox holds.
    #[must_use]
    pub const fn outbox_most(&self) -> usize {
        self.page
    }

    /// How many of an announcement's followers' inboxes it delivers to at
    /// once.
    #[must_use]
    pub const fn deliveries(&self) -> usize {
        self.deliveries
    }

    /// The actor, delivering to `deliveries` inboxes at once.
    #[must_use]
    pub fn delivering(self, deliveries: usize) -> Self {
        Self { deliveries, ..self }
    }

    /// The actor, its outbox paged `page` Notes at a time.
    #[must_use]
    pub fn paging(self, page: usize) -> Self {
        Self { page, ..self }
    }

    /// Whether `note`, kept as the Note of the announcement `key`, is this
    /// actor's.
    #[must_use]
    pub fn owns(&self, note: &str, key: &str) -> bool {
        let id = note_id(&self.origin, &self.route.actor, key);
        serde_json::from_str::<Value>(note)
            .is_ok_and(|note| note.get("id").and_then(Value::as_str) == Some(id.as_str()))
    }

    /// The `Accept` of `follow`, a `Follow` of this actor.
    #[must_use]
    pub fn accept(&self, follow: &Value) -> Value {
        accept(&self.origin, &self.route.actor, follow)
    }

    /// The `Follow` of this actor by `follower` whose id is `id`, as one
    /// waiting for the operator is answered.
    #[must_use]
    pub fn follow_of(&self, id: &str, follower: &str) -> Value {
        follow(&self.origin, &self.route.actor, id, follower)
    }

    /// The `Reject` of `follow`, a `Follow` of this actor its policy turns
    /// away.
    #[must_use]
    pub fn reject(&self, follow: &Value) -> Value {
        reject(&self.origin, &self.route.actor, follow)
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
        let header = signature_header(&key_id(&self.origin, &self.route.actor), &signature);
        let request = Request::new(Method::Post, Text::plain(inbox))
            .at_most(ANSWER)
            .header("User-Agent", Text::plain(USER_AGENT))
            .header("Content-Type", Text::plain(ACTIVITY_JSON))
            .header("Date", Text::plain(date))
            .header("Digest", Text::plain(delivered.digest))
            .header("Signature", Text::plain(header))
            .body(body);
        Ok(answered(&http.send(request).await?, self.waits))
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
fn answered(response: &Response, waits: Waits) -> Sent {
    if response.status == GONE {
        return Sent::Landed(Facts::default());
    }
    response
        .trouble(waits, None, "the inbox", String::new)
        .unwrap_or_else(|| Sent::Landed(Facts::default()))
}

/// What herald's ActivityPub actor can do: one Note per announcement,
/// delivered to its followers, and nothing it can read back, pin or edit.
#[must_use]
pub fn actor_profile() -> Profile {
    Profile {
        markup: ACTOR_MARKUP,
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
        symbols: herald_core::Symbols::DEFAULT,
    }
}
