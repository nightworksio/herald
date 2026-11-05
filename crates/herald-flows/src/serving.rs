use std::collections::BTreeMap;

use herald_config::{App, Config, Source};
use herald_core::{ACTIVITY_JSON, Addressed, Event, Key, Scheme, SourceName, digest, same};
use herald_dest_fediverse::ActivityPub;
use herald_ports::{Clock, CryptoError, Http, Keyring, Secret, Secrets, Signer, Store, Verifier};
use herald_sources_forge::{Forge, Hooked};

use crate::announce::{Announced, Outcome, Ports, announce, announce_within};
use crate::following::{FollowError, Followed, followed};
use crate::inbound::{Keys, Rejected, Verified, activity_verified, admitted, released, verified};
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
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(named, _)| named.eq_ignore_ascii_case(name))
            .map(|(_, value)| *value)
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
}

/// A request's reply, and the work it leaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    /// The reply.
    pub reply: Reply,
    /// The work to do once the reply is sent.
    pub job: Option<Job>,
}

/// What a job did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Worked {
    /// What each route did with the event.
    Announced(Vec<Announced>),
    /// What the activity did.
    Followed(Result<Followed, FollowError>),
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
}

fn said(status: u16, text: &str) -> Answered {
    replied(status, TEXT, text)
}

fn replied(status: u16, kind: &'static str, body: impl Into<Vec<u8>>) -> Answered {
    Answered {
        reply: Reply {
            status,
            kind,
            body: body.into(),
        },
        job: None,
    }
}

fn missing(what: &str) -> Answered {
    said(404, &format!("herald serves no {what} here"))
}

fn unmethodical() -> Answered {
    said(405, "herald does not answer this method here")
}

/// The reply to a request `rejected` keeps from its parser.
fn rejection(rejected: &Rejected) -> Answered {
    let status = match rejected {
        Rejected::Replayed => 409,
        Rejected::Unadmitted { .. } => 503,
        _ => 401,
    };
    said(status, &rejected.to_string())
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
fn actor<'a, H>(connected: &'a Connected<H>, name: &str) -> Option<&'a ActivityPub> {
    connected
        .routes
        .iter()
        .find_map(|(route, _)| match &route.reached {
            Reached::ActivityPub(actor) if actor.actor() == name => Some(actor),
            _ => None,
        })
}

/// Whether herald serves an actor on `host`.
fn hosts<H>(connected: &Connected<H>, host: &str) -> bool {
    connected.routes.iter().any(|(route, _)| {
        matches!(&route.reached, Reached::ActivityPub(actor) if actor.domain() == host)
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
        ("POST", SHARED_INBOX) => inboxed(serving, inbound).await,
        (_, HEALTH | WEBFINGER | SHARED_INBOX) => unmethodical(),
        (method, path) => {
            if let Some(rest) = path.strip_prefix(PROJECTS) {
                return match (method, rest.split('/').collect::<Vec<_>>().as_slice()) {
                    ("POST", [project, "events"]) => posted(serving, project, inbound).await,
                    (_, [_, "events"]) => unmethodical(),
                    _ => missing("such path"),
                };
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
                .and_then(|name| actor(serving.connected, name))
            else {
                return missing("such actor");
            };
            match (method, parts.as_slice()) {
                ("GET", [_]) => documented(serving, served).await,
                ("POST", [_, "inbox"]) => inboxed(serving, inbound).await,
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
        Err(error) => said(500, &format!("the actor's key cannot be read: {error}")),
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
        Err(error) => said(503, &error.to_string()),
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
    if let Err(rejected) = admitted(store, scheme, &verified, now).await {
        return rejection(&rejected);
    }
    match forge.hooked(inbound.headers, inbound.body, &options.repository) {
        Ok(Hooked::Release(event)) => Answered {
            reply: said(202, "herald announces the release").reply,
            job: Some(Job::Announce {
                source,
                event,
                scheme,
                verified,
            }),
        },
        Ok(Hooked::Nothing(why)) => said(200, why),
        Err(error) => {
            let _ = released(store, scheme, &verified).await;
            said(400, &error.to_string())
        }
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
    let token = inbound
        .header("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|token| !token.is_empty());
    let Some((name, app)) = token.and_then(|token| holder(serving.config, token)) else {
        return said(401, "herald knows no such token");
    };
    if !app.projects.iter().any(|named| named == project) {
        return said(403, "the token may not announce this project");
    }
    let now = serving.ports.clock.now();
    let event = match herald_sources_app::event(name, inbound.body, now) {
        Ok(event) => event,
        Err(unread) => return said(400, &unread.to_string()),
    };
    if event.project() != project {
        return said(
            400,
            &format!(
                "the event names the project \"{}\", not \"{project}\"",
                event.project()
            ),
        );
    }
    let verified = Verified {
        delivery: Some(event.identity().as_str().to_owned()),
        signer: Some(name.to_owned()),
    };
    match admitted(serving.ports.store, Scheme::App, &verified, now).await {
        Ok(()) => Answered {
            reply: said(202, "herald announces the event").reply,
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
) -> Answered {
    let host = inbound.header("host").unwrap_or_default();
    if !hosts(serving.connected, host) {
        return missing("actor on this host");
    }
    let target = match inbound.query {
        Some(query) => format!("post {}?{query}", inbound.path),
        None => format!("post {}", inbound.path),
    };
    let addressed = Addressed {
        target: &target,
        audience: host,
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
    if let Err(rejected) = admitted(serving.ports.store, Scheme::ActivityPub, &verified, now).await
    {
        return rejection(&rejected);
    }
    Answered {
        reply: said(202, "herald takes the activity").reply,
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
