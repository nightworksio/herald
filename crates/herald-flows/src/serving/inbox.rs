//! The inbox of each `herald` source a peer may push to (ADR-0054): one
//! envelope or tombstone, read only once it is no larger than its peer
//! may send, its signature by a key standing for a peer holds, its nonce
//! is new and it comes within the peer's rate, and the source grants the
//! peer `push` now; then taken as a pull takes it, its proof kept in the
//! source's cursor before it is answered. A push presenting a CI run's ID
//! token instead stands for the name its app does (ADR-0064). Nothing of a
//! request is kept before one of its sender's rates counts it; then it is
//! kept in the audit of its sender's day, never its body.

use std::collections::BTreeMap;
use std::sync::Arc;

use herald_config::Peer;
use herald_config::{Config, Federation};
use herald_core::{
    Action, AuditDay, Audited, Cursor, Grant, IdToken, Identity, Polled, Rate, Refusing,
    SourceName, Window, status,
};
use herald_kinds::{Pulling, Receive, Trust};
pub(super) use herald_peers::discovery::INBOX;
use herald_peers::request::{Request, checked};
use herald_ports::{
    Admitted, Algorithm, Cursors, Deliveries, DynVerifier, Erased, Http, Published, Signer, Store,
    Verifier, Versioned, Written,
};
use jiff::Timestamp;

use super::discovery::instance;
use super::jobs::Job;
use super::replies::{Answered, missing, text_reply};
use super::{Inbound, Serving};
use crate::announce::Stop;
use crate::peering::bounds;
use crate::poll::pulled::{pushing, signer};
use crate::verification::Admission;

/// What a host that is not the instance's domain has none of.
const NO_INBOX: &str = "herald inbox on this host";
/// What follows a peer's name in the window of the nonces replayed in its
/// name, which no peer's name holds.
const REPLAYS: &str = ".replays";
/// The namespace each pushing peer's nonces are admitted under.
const ADMITTED: &str = "inbox";
/// The answer to a push taken.
const TAKEN: &str = "herald took the envelope";
/// The answer to a request no peer signed.
const UNSIGNED: &str = "herald takes pushes signed by its peers only";
/// The answer to a push the source does not take from the peer.
const FORBIDDEN: &str = "herald does not take this push from the peer";
/// The answer to a body larger than the peer may send.
const LARGE: &str = "the push is larger than herald reads";
/// The answer to a peer past its rate.
const LIMITED: &str = "the peer pushes more often than herald takes";
/// The answer to a push herald cannot take now.
const UNAVAILABLE: &str = "herald cannot take the push now";

/// What a push was answered, and why.
#[derive(Debug)]
enum Verdict {
    Taken(Option<Box<Job>>, String, Option<Identity>),
    Unsigned(String),
    Forbidden(String),
    Large(String),
    Limited(&'static str),
    Unavailable(String),
}

impl Verdict {
    fn status(&self) -> u16 {
        match self {
            Self::Taken(..) => status::ACCEPTED,
            Self::Unsigned(_) => status::UNAUTHORIZED,
            Self::Forbidden(_) => status::FORBIDDEN,
            Self::Large(_) => status::CONTENT_TOO_LARGE,
            Self::Limited(_) => status::TOO_MANY_REQUESTS,
            Self::Unavailable(_) => status::UNAVAILABLE,
        }
    }

    fn reason(&self) -> &str {
        match self {
            Self::Taken(_, why, _)
            | Self::Unsigned(why)
            | Self::Forbidden(why)
            | Self::Large(why)
            | Self::Unavailable(why) => why,
            Self::Limited(why) => why,
        }
    }
}

/// Who sent a push, as far as its signature or ID token tells.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Sender {
    /// No one its signature names.
    Unknown,
    /// The key whose id it names, which no peer holds or whose signature
    /// does not hold.
    Key(String),
    /// The peer whose key signed it, the signature holding.
    Peer(String),
    /// The peer whose key signed it, or the name an app whose ID token
    /// it presented stands for, refused before any of its rates counted
    /// the request or once past one.
    Uncounted(String),
    /// The name an app stands for, whose ID token it presented.
    Run(Run),
}

/// A push presenting a CI run's ID token: the name its app stands for,
/// the token's issuer as `oidc <issuer>`, what the token said, and the
/// identity of the envelope it pushed, as taken where it was, and the
/// digest of its body.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Run {
    peer: String,
    keyid: String,
    evidence: BTreeMap<String, String>,
    identity: Option<String>,
    digest: String,
}

/// Who a push is kept in the audit as: a peer or none, the key or
/// issuer it named, and where it presented an ID token, the run it
/// presented.
type By<'r> = (Option<String>, Option<String>, Option<&'r Run>);

/// A push of a peer refused before it is granted.
#[derive(Debug)]
enum Refused {
    /// Counted within one of the peer's rates, or the first past one.
    Counted(Verdict),
    /// Refused before any of the peer's rates counted it, or once past one.
    Uncounted(Verdict),
}

/// The answer to a push into the source `name`, kept in the audit of its
/// sender's day.
pub(super) async fn pushed<
    S: Store,
    C: herald_ports::Clock,
    G: Signer,
    V: Verifier,
    H: Http,
    P: Http,
    K,
>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    inbound: &Inbound<'_>,
    name: &str,
) -> Answered {
    let Some(here) = instance(serving, inbound.host()) else {
        return missing(NO_INBOX);
    };
    let now = serving.ports.clock.now();
    let presented = inbound
        .header("authorization")
        .and_then(herald_core::bearer)
        .filter(|token| IdToken::shaped(token));
    let (sender, verdict) = match presented {
        Some(token) => presented::judged(serving, (inbound, &here), (name, token), now).await,
        None => judged(serving, inbound, name, now).await,
    };
    let kept = match &sender {
        _ if serving.admission == Admission::Again => Ok(()),
        Sender::Unknown | Sender::Uncounted(_) => Ok(()),
        Sender::Key(keyid) => {
            let rate = serving.config.federation.stranger_rate;
            match counted(serving, (AuditDay::STRANGERS, rate), now).await {
                Ok(Counted::Within) => {
                    let by = (None, Some(keyid.clone()), None);
                    audited(serving, (by, name), &verdict, now).await
                }
                Ok(Counted::FirstPast) => {
                    let marker = Verdict::Unsigned(format!(
                        "{}; past the strangers' rate, no more are kept in this window",
                        verdict.reason()
                    ));
                    let by = (None, Some(keyid.clone()), None);
                    audited(serving, (by, name), &marker, now).await
                }
                Ok(Counted::Past) => Ok(()),
                Err(unkept) => Err(unkept.reason().to_owned()),
            }
        }
        Sender::Peer(peer) => {
            let by = (Some(peer.clone()), None, None);
            audited(serving, (by, name), &verdict, now).await
        }
        Sender::Run(run) => {
            let taken = match &verdict {
                Verdict::Taken(_, _, Some(identity)) => Run {
                    identity: Some(Audited::named(identity.as_str())),
                    ..run.clone()
                },
                _ => run.clone(),
            };
            let by = (
                Some(run.peer.clone()),
                Some(run.keyid.clone()),
                Some(&taken),
            );
            audited(serving, (by, name), &verdict, now).await
        }
    };
    let status = verdict.status();
    let reason = match sender {
        Sender::Uncounted(peer) => format!("{peer}: {}", verdict.reason()),
        _ => verdict.reason().to_owned(),
    };
    let body = match status {
        status::ACCEPTED => TAKEN,
        status::UNAUTHORIZED => UNSIGNED,
        status::FORBIDDEN => FORBIDDEN,
        status::CONTENT_TOO_LARGE => LARGE,
        status::TOO_MANY_REQUESTS => LIMITED,
        _ => UNAVAILABLE,
    };
    let job = match verdict {
        Verdict::Taken(job, ..) => job.map(|job| *job),
        _ => None,
    };
    let refused = match (status == status::ACCEPTED, kept) {
        (true, Ok(())) => None,
        (true, Err(unkept)) => Some(format!("the audit was not kept: {unkept}")),
        (false, Ok(())) => Some(reason),
        (false, Err(unkept)) => Some(format!("{reason}; the audit was not kept: {unkept}")),
    };
    Answered {
        job,
        refused,
        ..text_reply(status, body)
    }
}

/// Who sent the push into `name`, and what it is answered.
async fn judged<S: Store, C, G, V: Verifier, H, P: Http, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    inbound: &Inbound<'_>,
    name: &str,
    now: Timestamp,
) -> (Sender, Verdict) {
    let config = serving.config;
    let largest = config
        .peers
        .values()
        .map(|peer| peer.most)
        .max()
        .unwrap_or_default();
    if inbound.body.len() > largest {
        return (
            Sender::Unknown,
            Verdict::Large(format!("{} bytes", inbound.body.len())),
        );
    }
    let fields: Vec<(String, String)> = inbound
        .headers
        .iter()
        .map(|(name, value)| ((*name).to_ascii_lowercase(), (*value).to_owned()))
        .collect();
    let query = inbound
        .query
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    let url = format!("https://{}{}{query}", inbound.host(), inbound.path);
    let request = Request {
        method: inbound.method,
        url: &url,
        fields: &fields,
        body: inbound.body,
    };
    let signed = match checked(&request, bounds(config, now)) {
        Ok(signed) => signed,
        Err(why) => return (Sender::Unknown, Verdict::Unsigned(why.to_string())),
    };
    let keyid = Audited::named(signed.keyid());
    let verifier: &dyn DynVerifier = serving.verifier;
    let reach = (config, serving.ports.store, serving.public);
    let (peer, kept, key) = match signer(reach, (verifier, now), signed.keyid()).await {
        Ok(Some(found)) => found,
        Ok(None) => {
            return (
                Sender::Key(keyid),
                Verdict::Unsigned("no peer holds the key".to_owned()),
            );
        }
        Err(stop) => return (Sender::Key(keyid), unavailable(&stop)),
    };
    if inbound.body.len() > kept.most {
        return (
            Sender::Unknown,
            Verdict::Large(format!(
                "more than the {} bytes {peer}, whose key it names, may send",
                kept.most
            )),
        );
    }
    let nonce = signed.nonce().to_owned();
    let skew = config.server.inbound.replay_window.as_secs();
    let dead = signed.expires().saturating_add(skew).saturating_add(1);
    let live = Timestamp::from_second(dead).unwrap_or(Timestamp::MAX);
    let shown = match signed.digested() {
        Ok(shown) => shown,
        Err(why) => return (Sender::Unknown, Verdict::Unsigned(why.to_string())),
    };
    if !Erased(verifier)
        .verify(Algorithm::Ed25519, &key, &shown.base, &shown.signature)
        .await
    {
        return (
            Sender::Key(keyid),
            Verdict::Unsigned("the signature does not hold".to_owned()),
        );
    }
    match admitted(serving, (peer, kept), (&nonce, live), now).await {
        Ok(()) => (
            Sender::Peer(peer.to_owned()),
            signed_in(serving, (peer, name), inbound.body, now).await,
        ),
        Err(Refused::Counted(verdict)) => (Sender::Peer(peer.to_owned()), verdict),
        Err(Refused::Uncounted(verdict)) => (Sender::Uncounted(peer.to_owned()), verdict),
    }
}

/// Why a peer's request past its rate is the first of its window to be
/// audited.
const PAST_RATE: &str = "past its rate; no more of its requests are kept in this window";
/// Why a peer's request past its rate, after the first, is refused.
const PAST: &str = "past its rate";
/// Why a peer's request is refused, and the first of its hold audited,
/// where too many of its requests refused past its rate are kept.
const HOLDING: &str = "past its rate and the refused requests kept; held, no more of its requests are kept until every one it made has expired";
/// Why a held peer's request is refused.
const HELD: &str = "held until every request it made has expired";
/// Why a replay past the peer's replay rate is the first of its window to
/// be audited.
const PAST_REPLAYS: &str =
    "past the rate of replayed nonces; no more replays are kept in this window";
/// Why a replay past the peer's replay rate, after the first, is refused.
const REPLAYED: &str = "past the rate of replayed nonces";

/// Whether the push of `peer`, whose nonce is `nonce` and whose
/// signature is live until just before `live`, is let through. A nonce
/// seen before counts against the peer's replays alone, never its own
/// rate, so no one replaying a request it captured spends the peer's; or
/// where the push is worked once more after it was answered, it is let
/// through uncounted. A new nonce counts against the peer's `rate` and is
/// admitted, so no request is taken twice, whether it is let through or
/// refused past the rate; one admitted meanwhile by a push racing it is a
/// replay. Past `[federation] refused_nonces` of those refused, the peer
/// is held until every request it made since has expired, nothing of it
/// taken and nothing more kept.
async fn admitted<S: Store, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    (peer, kept): (&str, &Peer),
    (nonce, live): (&str, Timestamp),
    now: Timestamp,
) -> Result<(), Refused> {
    let id = format!("{peer}/{nonce}");
    let deliveries = serving.ports.store.deliveries();
    let seen = deliveries
        .seen(ADMITTED, &id)
        .await
        .map_err(|error| Refused::Uncounted(Verdict::Unavailable(error.to_string())))?;
    if seen {
        return replayed(serving, peer, now).await;
    }
    let refused = match standing(serving, (peer, kept.rate), live, now)
        .await
        .map_err(Refused::Uncounted)?
    {
        Standing::Within => None,
        Standing::Held => return Err(Refused::Uncounted(Verdict::Limited(HELD))),
        Standing::Past(Refusing::Held, _) => {
            return Err(Refused::Counted(Verdict::Limited(HOLDING)));
        }
        Standing::Past(Refusing::Kept, true) => Some(Refused::Counted(Verdict::Limited(PAST_RATE))),
        Standing::Past(Refusing::Kept, false) => Some(Refused::Uncounted(Verdict::Limited(PAST))),
    };
    let admitted = deliveries
        .admit(ADMITTED, &id, now)
        .await
        .map_err(|error| Refused::Counted(Verdict::Unavailable(error.to_string())))?;
    match (refused, admitted) {
        (Some(refused), _) => Err(refused),
        (None, Admitted::Fresh) => Ok(()),
        (None, Admitted::Seen) => replayed(serving, peer, now).await,
    }
}

/// Where a request of a peer stands against its rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Standing {
    /// Within it.
    Within,
    /// Past it, as the refusal says, and whether it is the first past it
    /// in its window.
    Past(Refusing, bool),
    /// The peer is held.
    Held,
}

/// Where one more request of `peer`, live until `live`, stands against
/// its `rate` in the window the store keeps of it: where the peer is held,
/// its hold lengthened to `live`; past the rate, its nonce to be kept
/// where fewer than `[federation] refused_nonces` are, or the peer held.
/// Nothing is written where nothing changes.
async fn standing<S: Store, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    (peer, rate): (&str, Rate),
    live: Timestamp,
    now: Timestamp,
) -> Result<Standing, Verdict> {
    let (most, seconds) = (rate.count.get(), rate.per.length().as_secs());
    let refused = (
        serving.config.federation.refused_nonces,
        Federation::SIGNED_FOR_ALLOWED.as_secs(),
    );
    windowed(serving, peer, |held| {
        if held.holds(now) {
            let next = held.holding(live);
            return ((next != held).then_some(next), Standing::Held);
        }
        let next = held.counted(now, seconds);
        match next.count.checked_sub(most) {
            None | Some(0) => (Some(next), Standing::Within),
            Some(past) => {
                let (after, refusing) = next.refusing(now, refused, live);
                (Some(after), Standing::Past(refusing, past == 1))
            }
        }
    })
    .await
}

/// Where a push of `peer` whose nonce was seen before is let through: only
/// where it is worked once more after it was answered. Otherwise it is
/// refused, counted against the peer's replays.
async fn replayed<S: Store, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    peer: &str,
    now: Timestamp,
) -> Result<(), Refused> {
    if serving.admission == Admission::Again {
        return Ok(());
    }
    let replays = format!("{peer}{REPLAYS}");
    let rate = serving.config.federation.replay_rate;
    Err(
        match counted(serving, (&replays, rate), now)
            .await
            .map_err(Refused::Uncounted)?
        {
            Counted::Within => {
                Refused::Counted(Verdict::Unsigned("the nonce was seen before".to_owned()))
            }
            Counted::FirstPast => Refused::Counted(Verdict::Limited(PAST_REPLAYS)),
            Counted::Past => Refused::Uncounted(Verdict::Limited(REPLAYED)),
        },
    )
}

/// Where one more request of `peer` stands against its `rate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Counted {
    /// Within it.
    Within,
    /// The first past it in its window.
    FirstPast,
    /// Past it, after the first.
    Past,
}

/// Where one more request stands against `rate`, counted in the window
/// `name` the store keeps; one past the rate after the first is not
/// written.
async fn counted<S: Store, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    (name, rate): (&str, Rate),
    now: Timestamp,
) -> Result<Counted, Verdict> {
    let (most, seconds) = (rate.count.get(), rate.per.length().as_secs());
    windowed(serving, name, |held| {
        let next = held.counted(now, seconds);
        match next.count.checked_sub(most) {
            None | Some(0) => (Some(next), Counted::Within),
            Some(1) => (Some(next), Counted::FirstPast),
            Some(_) => (None, Counted::Past),
        }
    })
    .await
}

/// What `step` makes of the window `name` the store keeps, an empty one
/// where it keeps none: the window it gives written with compare-and-set,
/// where it gives one, before its answer is.
async fn windowed<S: Store, C, G, V, H, P, K, T>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    name: &str,
    step: impl Fn(Window) -> (Option<Window>, T),
) -> Result<T, Verdict> {
    let published = serving.ports.store.published();
    for _ in 0..serving.config.attempts.get() {
        let window = published
            .window(name)
            .await
            .map_err(|error| Verdict::Unavailable(error.to_string()))?;
        let held = window
            .as_ref()
            .map_or_else(Window::default, |window| window.value);
        let (next, answer) = step(held);
        let Some(next) = next else { return Ok(answer) };
        let expected = window.map(|window| window.version);
        match published.put_window(name, &next, expected).await {
            Ok(Written::Written(_)) => return Ok(answer),
            Ok(Written::Conflict) => {}
            Err(error) => return Err(Verdict::Unavailable(error.to_string())),
        }
    }
    Err(Verdict::Unavailable(
        "the peer's window kept changing".to_owned(),
    ))
}

/// What the signed push `body` of `peer` into the source `name` is
/// answered, where the source grants the peer `push` now: taken, its proof
/// held to the keys that stand for the peer and kept in the source's
/// cursor first.
async fn signed_in<S: Store, C, G, V: Verifier, H, P: Http, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    (peer, name): (&str, &str),
    body: &[u8],
    now: Timestamp,
) -> Verdict {
    let granting = match grant(serving.config, (peer, name), now) {
        Ok(granting) => granting,
        Err(refused) => return refused,
    };
    let verifier: &dyn DynVerifier = serving.verifier;
    let reach = (serving.config, serving.ports.store, serving.public);
    match pushing(reach, (verifier, now), peer).await {
        Ok(trust) => granted(serving, (peer, name), granting, (body, &trust), now).await,
        Err(stop) => unavailable(&stop),
    }
}

/// The inbox of the source `name` and what it grants `peer`, where it
/// grants the peer `push` at `now`.
///
/// # Errors
///
/// It grants none, or the grant has ended.
fn grant(
    config: &Config,
    (peer, name): (&str, &str),
    now: Timestamp,
) -> Result<(Arc<dyn Receive>, Grant), Verdict> {
    let Some((receiver, grant)) = config
        .sources
        .get(&SourceName::new(name))
        .and_then(|source| {
            let grant = source.grants()?.get(peer)?.clone();
            Some((source.receiver()?, grant))
        })
    else {
        return Err(Verdict::Forbidden(
            "the source grants the peer no push".to_owned(),
        ));
    };
    if !grant.allows(Action::Push, now) {
        return Err(Verdict::Forbidden(
            "the peer's grant to push to the source has ended".to_owned(),
        ));
    }
    Ok((receiver, grant))
}

/// What the push `body` of `peer` into the source `name`, whose inbox
/// `receiver` grants it `grant`, is answered: taken, its proof held to
/// `trust` and kept in the source's cursor first.
async fn granted<S: Store, C, G, V: Verifier, H, P: Http, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    (peer, name): (&str, &str),
    (receiver, grant): (Arc<dyn Receive>, Grant),
    (body, trust): (&[u8], &Trust),
    now: Timestamp,
) -> Verdict {
    let config = serving.config;
    let source = SourceName::new(name);
    let verifier: &dyn DynVerifier = serving.verifier;
    let pulling = Pulling {
        http: serving.public,
        verifier,
        specs: &config.kinds,
        now,
    };
    let cursors = serving.ports.store.cursors();
    let remembered = receiver.remembered();
    for _ in 0..config.attempts.get() {
        let kept = match cursors.cursor(&source).await {
            Ok(kept) => kept,
            Err(error) => return Verdict::Unavailable(error.to_string()),
        };
        let cursor = kept.as_ref().map(|kept| &kept.value);
        let polled = match receiver
            .received(
                (pulling, trust, cursor),
                (peer, &grant),
                (body, serving.admission == Admission::Again),
            )
            .await
        {
            Ok(polled) => polled,
            Err(why) => return Verdict::Forbidden(why),
        };
        if polled.proved.is_empty() {
            return Verdict::Taken(
                None,
                "no newer than the proof the source keeps".to_owned(),
                None,
            );
        }
        let next = cursor
            .cloned()
            .unwrap_or_default()
            .received(&polled, remembered);
        match cursors
            .put_cursor(&source, &next, kept.as_ref().map(|kept| kept.version))
            .await
        {
            Ok(Written::Written(_)) => {
                return Verdict::Taken(
                    job(source, &polled, cursor).map(Box::new),
                    "taken".to_owned(),
                    polled.proved.first().map(|proof| proof.identity.clone()),
                );
            }
            Ok(Written::Conflict) => {}
            Err(error) => return Verdict::Unavailable(error.to_string()),
        }
    }
    Verdict::Unavailable("the source's cursor kept changing".to_owned())
}

/// The work `polled`, pushed into `source` whose cursor was `cursor`,
/// leaves, where it leaves any.
fn job(source: SourceName, polled: &Polled, cursor: Option<&Cursor>) -> Option<Job> {
    let withdrawn = polled.withdrawn.first().map(|identity: &Identity| {
        (
            identity.clone(),
            cursor
                .and_then(|cursor| cursor.project_of(identity))
                .map(str::to_owned),
        )
    });
    let event = polled.events.first().cloned();
    (event.is_some() || withdrawn.is_some()).then_some(Job::Pushed {
        source,
        event,
        withdrawn,
    })
}

/// A push herald cannot take, as `stop` says.
fn unavailable(stop: &Stop) -> Verdict {
    Verdict::Unavailable(format!("{stop:?}"))
}

/// `verdict` on the push into the source `name` by `peer`, or a request
/// naming the key `keyid`, with what an ID token it presented said and
/// what it pushed, kept in the audit of its sender's day.
///
/// # Errors
///
/// The store cannot keep it.
async fn audited<S: Store, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    ((peer, keyid, run), name): (By<'_>, &str),
    verdict: &Verdict,
    now: Timestamp,
) -> Result<(), String> {
    let published = serving.ports.store.published();
    let owner = peer
        .clone()
        .unwrap_or_else(|| AuditDay::STRANGERS.to_owned());
    let entry = Audited {
        at: now,
        peer,
        keyid,
        action: Action::Push,
        resource: Audited::named(&format!("sources.{name}")),
        outcome: verdict.status(),
        reason: Audited::named(verdict.reason()),
        evidence: run.map(|run| run.evidence.clone()).unwrap_or_default(),
        identity: run.and_then(|run| run.identity.clone()),
        digest: run.map(|run| run.digest.clone()),
    };
    let fresh = AuditDay::of(&owner, now);
    for _ in 0..serving.config.attempts.get() {
        let kept: Option<Versioned<AuditDay>> = published
            .audit(&owner, &fresh.day)
            .await
            .map_err(|error| error.to_string())?;
        let expected = kept.as_ref().map(|kept| kept.version);
        let mut day = kept.map_or_else(|| fresh.clone(), |kept| kept.value);
        day.record(entry.clone());
        match published.put_audit(&day, expected).await {
            Ok(Written::Written(_)) => return Ok(()),
            Ok(Written::Conflict) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("the day's audit kept changing".to_owned())
}

mod presented;
