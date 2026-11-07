use std::collections::BTreeMap;

use herald_config::{App, Config, Source};
use herald_core::{
    ACTIVITY_JSON, Addressed, Asked, DestinationName, Event, Identity, Key, Scheme, SourceName,
    Standing, digest, same,
};
use herald_dest_fediverse::ActivityPub;
use herald_ports::{Clock, CryptoError, Http, Keyring, Secret, Secrets, Signer, Store, Verifier};
use herald_sources_forge::{Forge, Hooked};

use crate::announce::{Announced, Outcome, Ports, announce, announce_within};
use crate::commanded::{INTERACTIONS, MATRIX, appserviced, commanded};
use crate::following::{FollowError, Followed, followed};
use crate::inbound::{
    Admission, Keys, Rejected, Verified, activity_verified, admitted, released, verified,
};
use crate::interact::{Answer, interact, replied as replied_to};
use crate::interacting::Replying;
use crate::prepare::{Connected, PrepareError};
use crate::reached::Reached;

/// Where the server answers whether it is up.
pub const HEALTH: &str = "/healthz";
/// Where a forge delivers its webhooks, before the source's name.
pub const HOOKS: &str = "/hooks/";
/// Where apps post their projects' events: `/api/projects/<project>/events`.
pub const PROJECTS: &str = "/api/projects/";
/// Where WebFinger is asked.
const WEBFINGER: &str = "/.well-known/webfinger";
/// The inbox every actor herald serves shares.
const SHARED_INBOX: &str = "/inbox";
/// Where herald's actors are served, before an actor's name.
const USERS: &str = "/users/";
/// What WebFinger answers as.
const JRD: &str = "application/jrd+json";
/// What a reply in words is.
const TEXT: &str = "text/plain; charset=utf-8";
/// The media type of the HTTP API's answers.
const JSON: &str = "application/json";

/// An inbound HTTP request, as a driver hands it over.
#[derive(Debug, Clone, Copy)]
pub struct Inbound<'a> {
    /// The method, in capitals.
    pub method: &'a str,
    /// The path, without its query.
    pub path: &'a str,
    /// The query, without its `?`, where there is one.
    pub query: Option<&'a str>,
    /// Each header, as its name and value.
    pub headers: &'a [(&'a str, &'a str)],
    /// The raw body.
    pub body: &'a [u8],
}

impl Inbound<'_> {
    /// The value of the header `name`, in any case.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(named, _)| named.eq_ignore_ascii_case(name))
            .map(|(_, value)| *value)
    }

    fn host(&self) -> &str {
        self.header("host").unwrap_or_default()
    }
}

/// What herald answers a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// The status.
    pub status: u16,
    /// What the body is, as `Content-Type` names it.
    pub kind: &'static str,
    /// The body.
    pub body: Vec<u8>,
    /// How many seconds the sender is asked to wait before it asks again,
    /// as `Retry-After` names them.
    pub retry_after: Option<u32>,
}

/// Work a request leaves to be done once it is answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    /// `event`, delivered by a webhook of `source`, to announce; the
    /// delivery is released where a route stops (ADR-0013).
    Announce {
        /// The source the webhook is of.
        source: SourceName,
        /// The event it delivers.
        event: Event,
        /// The scheme it is signed under.
        scheme: Scheme,
        /// What its signature names.
        verified: Verified,
    },
    /// `event`, posted by the app named `app` to its HTTP API, to announce
    /// to the routes the app may announce to; the delivery is released where
    /// a route stops (ADR-0007, ADR-0013).
    App {
        /// The app's name.
        app: String,
        /// The event it posted.
        event: Event,
        /// What it is delivered as: the event's identity.
        verified: Verified,
    },
    /// An activity an inbox let through, to take.
    Follow {
        /// What its signature names.
        verified: Verified,
        /// The activity.
        body: Vec<u8>,
    },
    /// Commands given on `destination`, each to answer where it says.
    Interact {
        /// The destination they were given on.
        destination: DestinationName,
        /// Each command, with where its answer goes, each already answered
        /// as being worked where its destination waits for that.
        asked: Vec<(Asked, Replying)>,
    },
}

/// A request's reply, and the work it leaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    /// The reply.
    pub reply: Reply,
    /// The work to do once the reply is sent.
    pub job: Option<Job>,
    /// Why the request was refused, for the log where herald runs; its
    /// reply names no reason (ADR-0007).
    pub refused: Option<String>,
}

/// What a job did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Worked {
    /// What each route did with the event.
    Announced(Vec<Announced>),
    /// What the activity did.
    Followed(Result<Followed, FollowError>),
    /// What each command was answered, and whether the answer was sent.
    Interacted(Vec<Interacted>),
}

/// One command, answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interacted {
    /// The command.
    pub asked: Asked,
    /// What herald answered.
    pub answer: Answer,
    /// Whether the answer was sent, or else why not.
    pub sent: Result<(), String>,
}

/// The secret each forge source's webhooks are signed with, by the source.
#[derive(Debug, Clone, Default)]
pub struct Webhooks(BTreeMap<SourceName, Secret>);

/// The secret each forge source of `config` signs its webhooks with, read
/// through `secrets`.
///
/// # Errors
///
/// When a secret a source names is not set.
pub async fn webhooks<S: Secrets>(config: &Config, secrets: &S) -> Result<Webhooks, PrepareError> {
    let mut found = BTreeMap::new();
    for (name, source) in &config.sources {
        if let Source::Forge(_, options) = source
            && let Some(named) = &options.webhook
        {
            found.insert(name.clone(), secrets.secret(&named.secret).await?);
        }
    }
    Ok(Webhooks(found))
}

/// What a request is answered through.
#[derive(Debug)]
pub struct Serving<'a, S, C, G, V, H, P, K> {
    /// The config.
    pub config: &'a Config,
    /// Every route and source, connected.
    pub connected: &'a Connected<H>,
    /// The store, the clock and the signer.
    pub ports: &'a Ports<'a, S, C, G>,
    /// What checks signatures.
    pub verifier: &'a V,
    /// What reads actor documents, on any public address.
    pub public: &'a P,
    /// Where the keys activities are checked with are kept.
    pub keys: &'a Keys<'a, K>,
    /// The secret each forge source's webhooks are signed with.
    pub webhooks: &'a Webhooks,
    /// Whether a request's delivery is let through once, or again.
    pub admission: Admission,
}

pub(crate) fn said(status: u16, text: &str) -> Answered {
    replied(status, TEXT, text)
}

/// The refusal of a request admitted as `verified` whose body could not be
/// read for `reason`. Its delivery is released first, so the sender's next
/// delivery of it is admitted, not refused as a replay (ADR-0013).
pub(crate) async fn unread<S: Store>(
    store: &S,
    scheme: Scheme,
    verified: &Verified,
    reason: impl std::fmt::Display,
) -> Answered {
    let reason = match released(store, scheme, verified).await {
        Ok(()) => reason.to_string(),
        Err(error) => format!("{reason}; its delivery could not be released: {error}"),
    };
    refusal(400, reason)
}

/// The reply to a request refused for `reason`: a body that names no
/// reason, the same for every refusal of its kind, and the reason kept for
/// the log (ADR-0007).
/// A refusal answered `status` with herald's fixed body for it, its
/// `reason` kept for the log (ADR-0007).
pub fn refusal(status: u16, reason: impl Into<String>) -> Answered {
    let body = match status {
        409 => "herald has taken this request already",
        500.. => "herald cannot answer this request now",
        _ => "herald does not take this request",
    };
    Answered {
        refused: Some(reason.into()),
        ..said(status, body)
    }
}

/// How many seconds a request whose work found no place is asked to wait.
pub const BUSY_RETRY: u32 = 30;

/// The refusal of a request whose work found no place, asking its sender
/// to wait [`BUSY_RETRY`] seconds.
#[must_use]
pub fn busy() -> Answered {
    let refused = refusal(503, "the queue has no place for the work it leaves");
    Answered {
        reply: Reply {
            retry_after: Some(BUSY_RETRY),
            ..refused.reply
        },
        ..refused
    }
}

pub(crate) fn replied(status: u16, kind: &'static str, body: impl Into<Vec<u8>>) -> Answered {
    Answered {
        reply: Reply {
            status,
            kind,
            body: body.into(),
            retry_after: None,
        },
        job: None,
        refused: None,
    }
}

pub(crate) fn missing(what: &str) -> Answered {
    said(404, &format!("herald serves no {what} here"))
}

pub(crate) fn unmethodical() -> Answered {
    said(405, "herald does not answer this method here")
}

/// The reply to a request `rejected` keeps from its parser.
pub(crate) fn rejection(rejected: &Rejected) -> Answered {
    let status = match rejected {
        Rejected::Replayed => 409,
        Rejected::Unadmitted { .. } => 503,
        _ => 401,
    };
    refusal(status, rejected.to_string())
}

/// The scheme `forge` signs its webhooks under.
const fn scheme(forge: Forge) -> Scheme {
    match forge {
        Forge::GitHub => Scheme::GitHub,
        Forge::GitLab => Scheme::GitLab,
        Forge::Forgejo => Scheme::Forgejo,
    }
}

/// `text` with each `%` and its two hex digits read as the byte they write.
fn decoded(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.bytes();
    while let Some(byte) = rest.next() {
        if byte == b'%' {
            let digits = [rest.next()?, rest.next()?];
            let digits = std::str::from_utf8(&digits).ok()?;
            bytes.push(u8::from_str_radix(digits, 16).ok()?);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).ok()
}

/// The actor herald serves as `name`.
fn actor<'a, H>(connected: &'a Connected<H>, name: &str, host: &str) -> Option<&'a ActivityPub> {
    connected
        .routes
        .iter()
        .find_map(|(route, _)| match &route.reached {
            Reached::ActivityPub(actor) if actor.actor() == name && actor.domain() == host => {
                Some(actor)
            }
            _ => None,
        })
}

/// The domain from herald's config that is `host`, when herald serves an
/// actor on it.
fn domain<'a, H>(connected: &'a Connected<H>, host: &str) -> Option<&'a str> {
    connected
        .routes
        .iter()
        .find_map(|(route, _)| match &route.reached {
            Reached::ActivityPub(actor) if actor.domain() == host => Some(actor.domain()),
            _ => None,
        })
}

/// What WebFinger answers for the resource `query` asks for: an actor herald
/// serves, as `acct:<actor>@<domain>` or as its id.
fn fingered<H>(connected: &Connected<H>, query: Option<&str>) -> Answered {
    let resource = query
        .unwrap_or_default()
        .split('&')
        .find_map(|pair| pair.strip_prefix("resource="))
        .and_then(decoded)
        .unwrap_or_default();
    let found = connected
        .routes
        .iter()
        .find_map(|(route, _)| match &route.reached {
            Reached::ActivityPub(actor)
                if resource == format!("acct:{}@{}", actor.actor(), actor.domain())
                    || resource == actor.id() =>
            {
                Some(actor)
            }
            _ => None,
        });
    found.map_or_else(
        || missing("such actor"),
        |actor| replied(200, JRD, actor.finger()),
    )
}

/// The reply to `inbound`, and the work it leaves: health, forge webhooks,
/// and herald's ActivityPub actors, their Notes, inboxes and WebFinger
/// (ADR-0002). Every webhook and activity is verified and admitted before
/// it is parsed (ADR-0007, ADR-0013), and its work is left for after the
/// reply.
pub async fn answer<S: Store, C: Clock, G: Signer, V: Verifier, H, P: Http, K: Keyring>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    inbound: &Inbound<'_>,
) -> Answered {
    match (inbound.method, inbound.path) {
        ("GET", HEALTH) => said(200, "herald is up"),
        ("GET", WEBFINGER) => fingered(serving.connected, inbound.query),
        ("POST", SHARED_INBOX) => match domain(serving.connected, inbound.host()) {
            Some(audience) => inboxed(serving, inbound, audience).await,
            None => missing("actor on this host"),
        },
        (_, HEALTH | WEBFINGER | SHARED_INBOX) => unmethodical(),
        (method, path) => {
            if let Some(rest) = path.strip_prefix(PROJECTS) {
                return match (method, rest.split('/').collect::<Vec<_>>().as_slice()) {
                    ("POST", [project, "events"]) => posted(serving, project, inbound).await,
                    ("GET", [project, "events", id]) => {
                        standing(serving, project, id, inbound).await
                    }
                    (_, [_, "events"] | [_, "events", _]) => unmethodical(),
                    _ => missing("such path"),
                };
            }
            if let Some(destination) = path.strip_prefix(INTERACTIONS) {
                return if method == "POST" {
                    commanded(serving, destination, inbound).await
                } else {
                    unmethodical()
                };
            }
            if let Some((destination, rest)) = path
                .strip_prefix(MATRIX)
                .and_then(|rest| rest.split_once('/'))
            {
                return appserviced(serving, destination, rest, inbound).await;
            }
            if let Some(source) = path.strip_prefix(HOOKS) {
                return if method == "POST" {
                    hooked(serving, source, inbound).await
                } else {
                    unmethodical()
                };
            }
            let Some(rest) = path.strip_prefix(USERS) else {
                return missing("such path");
            };
            let parts: Vec<&str> = rest.split('/').collect();
            let Some(served) = parts
                .first()
                .and_then(|name| actor(serving.connected, name, inbound.host()))
            else {
                return missing("such actor");
            };
            match (method, parts.as_slice()) {
                ("GET", [_]) => documented(serving, served).await,
                ("POST", [_, "inbox"]) => inboxed(serving, inbound, served.domain()).await,
                ("GET", [_, "notes", key]) => noted(serving, served, key).await,
                (_, [_] | [_, "inbox"] | [_, "notes", _]) => unmethodical(),
                _ => missing("such path"),
            }
        }
    }
}

/// The actor document of `served`.
async fn documented<S, C, G: Signer, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    served: &ActivityPub,
) -> Answered {
    shown(served.document(serving.ports.signer).await)
}

/// The answer to an actor document that was or could not be built.
fn shown(document: Result<String, CryptoError>) -> Answered {
    match document {
        Ok(document) => replied(200, ACTIVITY_JSON, document),
        Err(error) => refusal(500, format!("the actor's key cannot be read: {error}")),
    }
}

/// The Note `served` published as the announcement `key`.
async fn noted<S: Store, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    served: &ActivityPub,
    key: &str,
) -> Answered {
    let Ok(parsed) = key.parse::<Key>() else {
        return missing("such note");
    };
    match serving.ports.store.note(&parsed).await {
        Ok(Some(note)) if served.owns(&note, key) => replied(200, ACTIVITY_JSON, note),
        Ok(_) => missing("such note"),
        Err(error) => refusal(503, error.to_string()),
    }
}

/// A webhook of the source `name`, verified and admitted, and the release
/// it publishes left to announce.
async fn hooked<S: Store, C: Clock, G, V: Verifier, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    name: &str,
    inbound: &Inbound<'_>,
) -> Answered {
    let source = SourceName::new(name);
    let (Some(Source::Forge(forge, options)), Some(secret)) = (
        serving.config.sources.get(&source),
        serving.webhooks.0.get(&source),
    ) else {
        return missing("webhook of such a source");
    };
    let scheme = scheme(*forge);
    let store = serving.ports.store;
    let now = serving.ports.clock.now();
    let checked = verified(
        serving.verifier,
        scheme,
        secret.expose(),
        inbound.headers,
        inbound.body,
        now,
    )
    .await;
    let verified = match checked {
        Ok(verified) => verified,
        Err(rejected) => return rejection(&rejected),
    };
    if let Err(rejected) = admitted(store, serving.admission, scheme, &verified, now).await {
        return rejection(&rejected);
    }
    match forge.hooked(inbound.headers, inbound.body, &options.repository) {
        Ok(Hooked::Release(event)) => Answered {
            reply: said(202, "herald announces the release").reply,
            refused: None,
            job: Some(Job::Announce {
                source,
                event,
                scheme,
                verified,
            }),
        },
        Ok(Hooked::Nothing(why)) => said(200, why),
        Err(error) => unread(store, scheme, &verified, error).await,
    }
}

/// An app's event posted to `project`: its token and the project are checked
/// before the event is read, the event is admitted once, and its
/// announcement is left to do (ADR-0007, ADR-0013).
async fn posted<S: Store, C: Clock, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    project: &str,
    inbound: &Inbound<'_>,
) -> Answered {
    let (name, _) = match scoped(serving.config, project, inbound) {
        Ok(scoped) => scoped,
        Err(refused) => return refused.answered(),
    };
    let now = serving.ports.clock.now();
    let event = match herald_sources_app::event(name, inbound.body, now) {
        Ok(event) => event,
        Err(unread) => return refusal(400, unread.to_string()),
    };
    if event.project() != project {
        return refusal(
            400,
            format!(
                "the event names the project \"{}\", not \"{project}\"",
                event.project()
            ),
        );
    }
    let verified = Verified {
        delivery: Some(event.identity().as_str().to_owned()),
        signer: Some(name.to_owned()),
    };
    match admitted(
        serving.ports.store,
        serving.admission,
        Scheme::App,
        &verified,
        now,
    )
    .await
    {
        Ok(()) => Answered {
            reply: said(202, "herald announces the event").reply,
            refused: None,
            job: Some(Job::App {
                app: name.to_owned(),
                event,
                verified,
            }),
        },
        Err(Rejected::Replayed) => said(200, "herald has this event already"),
        Err(rejected) => rejection(&rejected),
    }
}

/// Why an app's request is refused before it is read.
enum Refused {
    /// It presents no token herald knows.
    Token,
    /// Its token may not announce the project it names.
    Project,
}

impl Refused {
    fn answered(self) -> Answered {
        match self {
            Self::Token => refusal(401, "the request presents no token herald knows"),
            Self::Project => refusal(403, "the token may not announce this project"),
        }
    }
}

/// The app whose token `inbound` presents, where it may announce
/// `project`, or why it is refused.
fn scoped<'c>(
    config: &'c Config,
    project: &str,
    inbound: &Inbound<'_>,
) -> Result<(&'c str, &'c App), Refused> {
    let token = inbound
        .header("authorization")
        .and_then(herald_core::bearer);
    let Some((name, app)) = token.and_then(|token| holder(config, token)) else {
        return Err(Refused::Token);
    };
    if !app.projects.iter().any(|named| named == project) {
        return Err(Refused::Project);
    }
    Ok((name, app))
}

/// Where the announcement of the event its app gave `id` stands on each
/// route the app may announce to, read from the ledger (ADR-0015).
async fn standing<S: Store, C: Clock, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    project: &str,
    id: &str,
    inbound: &Inbound<'_>,
) -> Answered {
    let (name, app) = match scoped(serving.config, project, inbound) {
        Ok(scoped) => scoped,
        Err(refused) => return refused.answered(),
    };
    let identity = Identity::new(herald_sources_app::identity(name, id));
    let now = serving.ports.clock.now();
    let mut routes = Vec::new();
    for route in &app.routes {
        let key = Key::derive(route, &identity);
        let standing = match serving.ports.store.entry(&key).await {
            Ok(Some(entry)) => entry.value.standing(now),
            Ok(None) => Standing::Pending,
            Err(error) => return refusal(503, error.to_string()),
        };
        routes.push((route.as_str(), standing));
    }
    replied(
        200,
        JSON,
        herald_sources_app::standings(id, project, &routes),
    )
}

/// The app one of whose tokens is `token`, its SHA-256 compared with every
/// hash every app keeps, each in constant time (ADR-0007).
fn holder<'c>(config: &'c Config, token: &str) -> Option<(&'c str, &'c App)> {
    let presented = digest(token);
    let mut found = None;
    for (name, app) in &config.apps {
        for held in &app.tokens {
            if same(held.as_bytes(), presented.as_bytes()) {
                found = Some((name.as_str(), app));
            }
        }
    }
    found
}

/// An activity delivered to an inbox herald serves, verified and admitted,
/// and left to take.
async fn inboxed<S: Store, C: Clock, G, V: Verifier, H, P: Http, K: Keyring>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    inbound: &Inbound<'_>,
    audience: &str,
) -> Answered {
    let target = match inbound.query {
        Some(query) => format!("post {}?{query}", inbound.path),
        None => format!("post {}", inbound.path),
    };
    let addressed = Addressed {
        target: &target,
        audience,
    };
    let now = serving.ports.clock.now();
    let checked = activity_verified(
        serving.verifier,
        serving.public,
        serving.keys,
        addressed,
        inbound.headers,
        inbound.body,
        now,
    )
    .await;
    let verified = match checked {
        Ok(verified) => verified,
        Err(rejected) => return rejection(&rejected),
    };
    if let Err(rejected) = admitted(
        serving.ports.store,
        serving.admission,
        Scheme::ActivityPub,
        &verified,
        now,
    )
    .await
    {
        return rejection(&rejected);
    }
    Answered {
        reply: said(202, "herald takes the activity").reply,
        refused: None,
        job: Some(Job::Follow {
            verified,
            body: inbound.body.to_vec(),
        }),
    }
}

/// Releases the delivery `job` came by, as it will not be done, so its
/// sender's next delivery of it is admitted (ADR-0013).
pub async fn unworked<S: Store, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    job: &Job,
) {
    let (scheme, verified) = match job {
        Job::Announce {
            scheme, verified, ..
        } => (*scheme, verified),
        Job::App { verified, .. } => (Scheme::App, verified),
        Job::Follow { verified, .. } => (Scheme::ActivityPub, verified),
        Job::Interact { .. } => return,
    };
    let _ = released(serving.ports.store, scheme, verified).await;
}

/// `job` done, under the run name `run`: a delivery whose work stopped or
/// failed is released, so its sender's next delivery of it is admitted
/// (ADR-0013).
pub async fn work<S: Store, C: Clock, G: Signer, V, H: Http, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    job: Job,
    run: &str,
) -> Worked {
    let store = serving.ports.store;
    match job {
        Job::Announce {
            source,
            event,
            scheme,
            verified,
        } => {
            let announced = announce(
                serving.config,
                serving.connected,
                serving.ports,
                &source,
                &event,
                run,
            )
            .await;
            if announced
                .iter()
                .any(|announced| matches!(announced.outcome, Outcome::Stopped { .. }))
            {
                let _ = released(store, scheme, &verified).await;
            }
            Worked::Announced(announced)
        }
        Job::App {
            app,
            event,
            verified,
        } => {
            let within = serving
                .config
                .apps
                .get(&app)
                .map(|app| app.routes.as_slice())
                .unwrap_or_default();
            let announced = announce_within(
                serving.config,
                serving.connected,
                serving.ports,
                &SourceName::new(&app),
                &event,
                Some(within),
                run,
            )
            .await;
            if announced
                .iter()
                .any(|announced| matches!(announced.outcome, Outcome::Stopped { .. }))
            {
                let _ = released(store, Scheme::App, &verified).await;
            }
            Worked::Announced(announced)
        }
        Job::Follow { verified, body } => {
            let taken = followed(
                serving.connected,
                store,
                serving.ports.signer,
                serving.ports.clock.now(),
                &verified,
                &body,
            )
            .await;
            if taken.is_err() {
                let _ = released(store, Scheme::ActivityPub, &verified).await;
            }
            Worked::Followed(taken)
        }
        Job::Interact { destination, asked } => {
            let mut interacted = Vec::new();
            for (asked, replying) in asked {
                let answer =
                    interact(serving.config, serving.connected, serving.ports, &asked).await;
                let sent = replied_to(serving.connected, &destination, &replying, &answer).await;
                interacted.push(Interacted {
                    asked,
                    answer,
                    sent,
                });
            }
            Worked::Interacted(interacted)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_percent_and_two_hex_digits_read_as_their_byte() {
        assert_eq!(
            decoded("acct%3Aana%40social.example").as_deref(),
            Some("acct:ana@social.example")
        );
        assert_eq!(decoded("plain").as_deref(), Some("plain"));
        for broken in ["%", "%4", "%zz", "%ff"] {
            assert_eq!(decoded(broken), None, "{broken}");
        }
    }

    #[test]
    fn each_forge_signs_under_its_own_scheme() {
        assert_eq!(scheme(Forge::GitHub), Scheme::GitHub);
        assert_eq!(scheme(Forge::GitLab), Scheme::GitLab);
        assert_eq!(scheme(Forge::Forgejo), Scheme::Forgejo);
    }

    #[test]
    fn a_rejected_request_is_answered_by_why() {
        assert_eq!(rejection(&Rejected::Replayed).reply.status, 409);
        assert_eq!(
            rejection(&Rejected::Unadmitted {
                reason: "down".to_owned()
            })
            .reply
            .status,
            503
        );
        assert_eq!(rejection(&Rejected::Forged).reply.status, 401);
    }
}
