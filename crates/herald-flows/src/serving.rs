use std::collections::BTreeMap;

use herald_config::{App, Config};
use herald_core::{
    Asked, DestinationName, Event, Identity, Key, Scheme, SourceName, Standing, digest, same,
};
use herald_kinds::Hooked;
use herald_kinds::{Endpoint, Replying};
use herald_ports::{Clock, Http, Keyring, Ledger, Secret, Secrets, Signer, Store, Verifier};

use crate::announce::{Announced, Outcome, Ports, announce, announce_within};
use crate::commanded::{INTERACTIONS, MATRIX, commanded};
use crate::following::{FollowError, Followed, followed};
use crate::inbound::{Admission, Checking, Keys, Rejected, Verified, admitted, released, verified};
use crate::interact::{Answer, interact, replied as replied_to};
use crate::prepare::{Connected, PrepareError};

mod actors;

use actors::{actor, counted, documented, domain, fingered, inboxed, noted, outboxed};

/// Where the server answers whether it is up.
pub const HEALTH: &str = "/healthz";
/// What the server answers at [`HEALTH`].
const UP: &str = "herald is up";
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

impl Worked {
    /// Whether the work stopped before it was done: a route stopped, or the
    /// activity was not taken.
    #[must_use]
    pub fn stopped(&self) -> bool {
        match self {
            Self::Announced(announced) => announced
                .iter()
                .any(|announced| matches!(announced.outcome, Outcome::Stopped { .. })),
            Self::Followed(taken) => taken.is_err(),
            Self::Interacted(_) => false,
        }
    }
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

impl Job {
    /// The scheme and identity of the delivery the job came by, where it
    /// came by one to release.
    #[must_use]
    pub fn delivery(&self) -> Option<(Scheme, &Verified)> {
        match self {
            Self::Announce {
                scheme, verified, ..
            } => Some((*scheme, verified)),
            Self::App { verified, .. } => Some((herald_sources_app::SCHEME, verified)),
            Self::Follow { verified, .. } => Some((herald_dest_fediverse::ACTOR_SCHEME, verified)),
            Self::Interact { .. } => None,
        }
    }
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
        if let Some(named) = source.webhook() {
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

/// The reply to `method` on `target` that needs no composed herald, where
/// there is one: whether it is up, at [`HEALTH`] under `base`. A host that
/// composes herald per request answers it once its config loads, before it
/// opens a store.
#[must_use]
pub fn uncomposed(method: &str, target: &str, base: &str) -> Option<Reply> {
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    (method == "GET" && under(base, path) == Some(HEALTH)).then(|| said(200, UP).reply)
}

/// `path` as herald routes it, where it lies under `base`, the path every
/// endpoint is served under: `base` taken off, and the root where nothing
/// is left.
#[must_use]
pub fn under<'a>(base: &str, path: &'a str) -> Option<&'a str> {
    let rest = path.strip_prefix(base)?;
    match rest {
        "" => Some("/"),
        rest if rest.starts_with('/') => Some(rest),
        _ => None,
    }
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

/// The refusal of a request whose work found no place, asking its sender
/// to wait `after`, in whole seconds rounded up.
#[must_use]
pub fn busy(after: jiff::SignedDuration) -> Answered {
    let refused = refusal(503, "the queue has no place for the work it leaves");
    let whole = after
        .as_secs()
        .saturating_add(i64::from(after.subsec_nanos() > 0));
    Answered {
        reply: Reply {
            retry_after: Some(u32::try_from(whole.max(1)).unwrap_or(u32::MAX)),
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

/// The reply to `inbound`, and the work it leaves: health, forge webhooks,
/// and herald's ActivityPub actors, their Notes, inboxes and WebFinger
/// (ADR-0002). Every webhook and activity is verified and admitted before
/// it is parsed (ADR-0007, ADR-0013), and its work is left for after the
/// reply.
pub async fn answer<S: Store, C: Clock, G: Signer, V: Verifier, H, P: Http, K: Keyring>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    inbound: &Inbound<'_>,
) -> Answered {
    let Some(path) = under(&serving.config.server.base, inbound.path) else {
        return missing("such path");
    };
    match (inbound.method, path) {
        ("GET", HEALTH) => said(200, UP),
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
                    commanded(serving, Endpoint::Posted, destination, "", inbound).await
                } else {
                    unmethodical()
                };
            }
            if let Some((destination, rest)) = path
                .strip_prefix(MATRIX)
                .and_then(|rest| rest.split_once('/'))
            {
                return commanded(serving, Endpoint::Pushed, destination, rest, inbound).await;
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
            let Some((served, destination)) = parts
                .first()
                .and_then(|name| actor(serving.connected, name, inbound.host()))
            else {
                return missing("such actor");
            };
            match (method, parts.as_slice()) {
                ("GET", [_]) => documented(serving, served).await,
                ("POST", [_, "inbox"]) => inboxed(serving, inbound, served.domain()).await,
                ("GET", [_, "notes", key]) => noted(serving, served, key).await,
                ("GET", [_, "outbox"]) => outboxed(serving, served, inbound.query).await,
                ("GET", [_, "followers"]) => counted(serving, served, destination).await,
                (_, [_] | [_, "inbox" | "outbox" | "followers"] | [_, "notes", _]) => {
                    unmethodical()
                }
                _ => missing("such path"),
            }
        }
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
    let Some((hooking, scheme, secret)) = serving
        .config
        .sources
        .get(&source)
        .zip(serving.webhooks.0.get(&source))
        .and_then(|(hooking, secret)| hooking.scheme().map(|scheme| (hooking, scheme, secret)))
    else {
        return missing("webhook of such a source");
    };
    let store = serving.ports.store;
    let now = serving.ports.clock.now();
    let checked = verified(
        serving.verifier,
        scheme,
        secret.expose(),
        inbound.headers,
        inbound.body,
        Checking {
            now,
            bounds: &serving.config.server.inbound,
        },
    )
    .await;
    let verified = match checked {
        Ok(verified) => verified,
        Err(rejected) => return rejection(&rejected),
    };
    if let Err(rejected) = admitted(store, serving.admission, scheme, &verified, now).await {
        return rejection(&rejected);
    }
    match hooking.hooked(inbound.headers, inbound.body) {
        Ok(Hooked::Event(event)) => Answered {
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
    let event = match herald_sources_app::event(name, inbound.body, now, &serving.config.kinds) {
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
        herald_sources_app::SCHEME,
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
        let standing = match serving.ports.store.ledger().entry(&key).await {
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

/// Releases the delivery `job` came by, as it will not be done, so its
/// sender's next delivery of it is admitted (ADR-0013).
pub async fn unworked<S: Store, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    job: &Job,
) {
    if let Some((scheme, verified)) = job.delivery() {
        let _ = released(serving.ports.store, scheme, verified).await;
    }
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
    let delivery = job
        .delivery()
        .map(|(scheme, verified)| (scheme, verified.clone()));
    let worked = match job {
        Job::Announce { source, event, .. } => {
            let announced = announce(
                serving.config,
                serving.connected,
                serving.ports,
                &source,
                &event,
                run,
            )
            .await;
            Worked::Announced(announced)
        }
        Job::App { app, event, .. } => {
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
            Worked::Announced(announced)
        }
        Job::Follow { verified, body } => {
            let taken = followed(
                serving.connected,
                store,
                serving.ports.signer,
                Checking {
                    now: serving.ports.clock.now(),
                    bounds: &serving.config.server.inbound,
                },
                &verified,
                &body,
            )
            .await;
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
    };
    if worked.stopped()
        && let Some((scheme, verified)) = delivery
    {
        let _ = released(store, scheme, &verified).await;
    }
    worked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_whether_it_is_up_is_answered_uncomposed() {
        let up = uncomposed("GET", "/healthz", "").unwrap();
        assert_eq!((up.status, up.body.as_slice()), (200, UP.as_bytes()));
        assert_eq!(uncomposed("GET", "/healthz?probe=1", ""), Some(up.clone()));
        assert_eq!(uncomposed("POST", "/healthz", ""), None);
        assert_eq!(uncomposed("GET", "/healthz/more", ""), None);
        assert_eq!(uncomposed("GET", "/inbox", ""), None);
        assert_eq!(uncomposed("GET", "/herald/healthz", "/herald"), Some(up));
        assert_eq!(uncomposed("GET", "/healthz", "/herald"), None);
        assert_eq!(uncomposed("GET", "/heraldhealthz", "/herald"), None);
    }

    #[test]
    fn a_path_is_routed_where_it_lies_under_the_base() {
        assert_eq!(under("", "/hooks/gate"), Some("/hooks/gate"));
        assert_eq!(under("/herald", "/herald/hooks/gate"), Some("/hooks/gate"));
        assert_eq!(under("/herald", "/herald"), Some("/"));
        assert_eq!(under("/herald", "/heraldry/hooks"), None);
        assert_eq!(under("/herald", "/hooks/gate"), None);
    }

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
