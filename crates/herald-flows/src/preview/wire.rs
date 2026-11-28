//! The requests a route's first post sends, as its destination's own
//! request builder writes them, sent nowhere (ADR-0095).

use std::future::{Future, ready};
use std::sync::{Arc, Mutex, PoisonError};

use herald_config::{Config, RouteConfig};
use herald_core::{
    Algorithm, Announcement, Entry, Event, Facts, Pinning, RouteId, SecretRef, Step, Visibility,
    itself, status, steps,
};
use herald_kinds::{Proving, Reached, Signing};
use herald_peers::discovery::Instance;
use herald_peers::jcs::canonical;
use herald_ports::{CryptoError, Http, HttpError, Request, Response, Secret, Signer};

use crate::announce::{PRIVATE, UNSHARED};

/// The id a run that sends nothing reads from every answer: of the post,
/// its thread and each message a step made.
pub const DRY_ID: &str = "0";

/// Why a route that delivers to followers, of a destination that
/// publishes no document, shows no request.
pub const FOLLOWED: &str = "herald delivers it to each follower's inbox, and preview knows no follower, so it shows no request";
/// Why a route whose event is too long for an envelope shows none.
pub const UNENVELOPED: &str =
    "its event is too long to put in an envelope, so herald publishes none";
/// Why a route whose destination the config does not reach shows no
/// request.
pub const UNREACHED: &str =
    "its destination cannot be reached with its options, so it shows no request";

/// An `Http` that sends nothing: it keeps each request it is handed, in
/// order, and answers each with a success naming [`DRY_ID`], as the id of
/// what it made and as a token a sign-in gives.
#[derive(Debug, Clone, Default)]
pub struct Recording {
    kept: Arc<Mutex<Vec<Request>>>,
}

impl Recording {
    /// Each request kept since this was last asked, in the order handed.
    #[must_use]
    pub fn taken(&self) -> Vec<Request> {
        std::mem::take(&mut *self.kept.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl Http for Recording {
    fn send(&self, request: Request) -> impl Future<Output = Result<Response, HttpError>> + Send {
        self.kept
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request);
        ready(Ok(Response {
            status: status::OK,
            headers: Vec::new(),
            body: format!(r#"{{"id":"{DRY_ID}","access_token":"{DRY_ID}","expires_in":3600}}"#)
                .into_bytes(),
        }))
    }
}

/// A signer that signs nothing, for the destinations that sign no request.
struct Unsigning;

impl Signer for Unsigning {
    fn sign(
        &self,
        _: Algorithm,
        key: &Secret,
        _: &[u8],
    ) -> impl Future<Output = Result<Vec<u8>, CryptoError>> + Send {
        ready(Err(CryptoError::BadKey {
            name: key.name().to_owned(),
        }))
    }

    fn public(
        &self,
        algorithm: Algorithm,
        key: &Secret,
    ) -> impl Future<Output = Result<Vec<u8>, CryptoError>> + Send {
        self.sign(algorithm, key, &[])
    }
}

/// What a route's first post sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wire {
    /// Each step it takes, in order, with each request the step sends.
    Sent(Vec<(Step, Vec<Request>)>),
    /// The document herald publishes in place of a post: served by one of
    /// its actors or feeds, or pushed to a peer.
    Served(Served),
    /// It sends no request to a platform, for the reason given.
    Unsent(&'static str),
}

/// A document herald publishes, as it publishes it, with its proof left
/// out and named (ADR-0095).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Served {
    /// Who publishes it.
    pub by: Publisher,
    /// The document, as JSON, without its proof.
    pub document: String,
    /// What proves it or signs the requests that carry it.
    pub proof: Proof,
}

/// Who publishes a document herald serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Publisher {
    /// The actor of this id, in its outbox, delivering it to each
    /// follower's inbox.
    Actor(String),
    /// Herald's own feed, which anyone reads.
    Feed,
    /// The push to the peer of this name.
    Peer(String),
}

/// What proves a document herald serves, by name alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proof {
    /// The key the secret of this name holds.
    Key(String),
    /// A CI run's ID token, which the peer proves.
    IdToken,
}

impl Proof {
    /// How it is named: a key as `«secret:NAME»`, a token as what it is.
    #[must_use]
    pub fn named(&self) -> String {
        match self {
            Self::Key(name) => Secret::new(name.clone(), String::new()).redacted(),
            Self::IdToken => crate::announce::ASKED.to_owned(),
        }
    }
}

/// The secret `reference` names, holding no value: shown as `«secret:NAME»`
/// wherever a value would be.
fn placeholder(reference: &SecretRef) -> Secret {
    Secret::new(reference.secret.clone(), String::new())
}

/// What the route `id` of `config` sends to post `announcement` first:
/// each step its delivery asks of its destination, written by the
/// destination's own request builder, every secret a placeholder that holds
/// no value, every id an earlier step's answer gives [`DRY_ID`], and nothing
/// sent (ADR-0095). Where its pins are kept in the store, it unpins
/// nothing: a route's first post finds no pin of its own there.
pub async fn wired(
    config: &Config,
    id: &RouteId,
    (announcement, event): (&Announcement, &Event),
) -> Wire {
    let Some((route, reached, credential)) = reach(config, id) else {
        return Wire::Unsent(UNREACHED);
    };
    if reached.follows().is_some() {
        return reached.published(announcement).map_or(
            Wire::Unsent(FOLLOWED),
            |(actor, document)| {
                Wire::Served(Served {
                    by: Publisher::Actor(actor.to_owned()),
                    document,
                    proof: Proof::Key(credential.secret.clone()),
                })
            },
        );
    }
    if let Some(wire) = envelope(config, &reached, event) {
        return wire;
    }
    let delivery = &route.route.delivery;
    let parts = u32::try_from(announcement.messages()).unwrap_or(u32::MAX);
    let recording = Recording::default();
    let landed = Facts {
        message: Some(DRY_ID.to_owned()),
        thread: Some(DRY_ID.to_owned()),
        permalink: None,
    };
    let mut entry = Entry::new(announcement.key);
    let mut sent = Vec::new();
    for step in steps(delivery, &reached.profile().features, parts, false) {
        let signing = Signing {
            signer: &Unsigning,
            now: announcement.occurred,
        };
        if step != Step::Unpin || delivery.pin != Some(Pinning::Store) {
            drop(
                reached
                    .send(&recording, signing, &step, announcement, delivery, &entry)
                    .await,
            );
        }
        entry = entry.landed(step.clone(), landed.clone());
        sent.push((step, recording.taken()));
    }
    Wire::Sent(sent)
}

/// The envelope the route `id` of `config` publishes `event` in, where it
/// publishes each event as one rather than rendering it: served by
/// herald's own feed or pushed to a peer (ADR-0095).
#[must_use]
pub fn enveloped(config: &Config, id: &RouteId, event: &Event) -> Wire {
    reach(config, id)
        .and_then(|(_, reached, _)| envelope(config, &reached, event))
        .unwrap_or(Wire::Unsent(UNREACHED))
}

/// The route `id` of `config`, reached with placeholder secrets, and the
/// secret its destination's credential names.
fn reach<'a>(
    config: &'a Config,
    id: &RouteId,
) -> Option<(&'a RouteConfig, Reached, &'a SecretRef)> {
    let route = config.routes.iter().find(|route| route.route.id == *id)?;
    let destination = config.destinations.get(&route.route.destination)?;
    let credential = config.credential(destination)?;
    let read = destination.reads().into_iter().map(placeholder).collect();
    let reached = destination.reach_reading(
        &route.options,
        &config.server.base,
        (placeholder(credential), read),
    )?;
    Some((route, reached, credential))
}

/// The envelope `reached` publishes `event` in, where it is a feed of
/// herald's own or a push to a peer; or why it publishes none.
fn envelope(config: &Config, reached: &Reached, event: &Event) -> Option<Wire> {
    if let Some(fed) = reached.feeds() {
        if seen(event) != Visibility::Public {
            return Some(Wire::Unsent(PRIVATE));
        }
        return Some(served(
            enveloped_as(config, (event, Visibility::Public), fed.feeding.envelope),
            Publisher::Feed,
            Proof::Key(fed.key.name().to_owned()),
        ));
    }
    let pushed = reached.pushes()?;
    if seen(event) != Visibility::Public && !pushed.private {
        return Some(Wire::Unsent(UNSHARED));
    }
    let proof = match pushed.proving {
        Proving::Key(key) => Proof::Key(key.name().to_owned()),
        Proving::IdToken { .. } => Proof::IdToken,
    };
    Some(served(
        enveloped_as(config, (event, seen(event)), pushed.envelope),
        Publisher::Peer(pushed.peer.to_owned()),
        proof,
    ))
}

/// Who may see `event`: as it is bounded, and anyone where it is not.
fn seen(event: &Event) -> Visibility {
    event.bounded_by().unwrap_or(Visibility::Public)
}

/// The envelope `config`'s instance publishes `event` in, seen as `seen`,
/// as canonical JSON without its proof, within `most` bytes; none where
/// the event is too long for one.
fn enveloped_as(
    config: &Config,
    (event, seen): (&Event, Visibility),
    most: usize,
) -> Option<String> {
    let instance = Instance {
        domain: config.federation.domain.as_deref().unwrap_or_default(),
        base: config.instance_base(),
        software: itself::NAME,
        version: itself::VERSION,
        repository: herald_core::repository!(),
    };
    let envelope = instance
        .envelope((event, seen), config.kinds.get(event.kind()), most)
        .ok()?;
    canonical(&envelope).ok()
}

/// `document`, published by `by` and proved by `proof`; or why none is,
/// where there is none.
fn served(document: Option<String>, by: Publisher, proof: Proof) -> Wire {
    document.map_or(Wire::Unsent(UNENVELOPED), |document| {
        Wire::Served(Served {
            by,
            document,
            proof,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signer_of_a_preview_signs_nothing_and_names_the_key() {
        let key = Secret::new("HERALD_ACTOR_KEY", String::new());
        let refused = CryptoError::BadKey {
            name: "HERALD_ACTOR_KEY".to_owned(),
        };
        assert_eq!(
            pollster::block_on(Unsigning.sign(Algorithm::RsaSha256, &key, b"message")),
            Err(refused.clone())
        );
        assert_eq!(
            pollster::block_on(Unsigning.public(Algorithm::RsaSha256, &key)),
            Err(refused)
        );
    }
}
