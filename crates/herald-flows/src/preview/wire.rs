//! The requests a route's first post sends, as its destination's own
//! request builder writes them, sent nowhere (ADR-0095).

use std::future::{Future, ready};
use std::sync::{Arc, Mutex, PoisonError};

use herald_config::Config;
use herald_core::{
    Algorithm, Announcement, Entry, Facts, Pinning, RouteId, SecretRef, Step, status, steps,
};
use herald_kinds::Signing;
use herald_ports::{CryptoError, Http, HttpError, Request, Response, Secret, Signer};

/// The id a run that sends nothing reads from every answer: of the post,
/// its thread and each message a step made.
pub const DRY_ID: &str = "0";

/// Why a route that delivers to followers shows no request.
pub const FOLLOWED: &str = "herald delivers it to each follower's inbox, and preview knows no follower, so it shows no request";
/// Why a route to one of herald's own feeds shows no request.
pub const FED: &str =
    "herald serves it in its own feed rather than sending it, so it shows no request";
/// Why a route that pushes to a peer shows no request.
pub const PUSHED: &str = "herald pushes it to a peer's inbox, signed by the instance's key, which preview never reads, so it shows no request";
/// Why a route whose destination the config does not reach shows no
/// request.
pub const UNREACHED: &str =
    "its destination cannot be reached with its options, so it shows no request";

/// An `Http` that sends nothing: it keeps each request it is handed, in
/// order, and answers each with a success naming [`DRY_ID`].
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
            body: format!(r#"{{"id":"{DRY_ID}"}}"#).into_bytes(),
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
    /// It sends no request to a platform, for the reason given.
    Unsent(&'static str),
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
pub async fn wired(config: &Config, id: &RouteId, announcement: &Announcement) -> Wire {
    let route = config.routes.iter().find(|route| route.route.id == *id);
    let reached = route.and_then(|route| {
        let destination = config.destinations.get(&route.route.destination)?;
        let credential = placeholder(config.credential(destination)?);
        let read = destination.reads().into_iter().map(placeholder).collect();
        destination.reach_reading(&route.options, &config.server.base, (credential, read))
    });
    let (Some(route), Some(reached)) = (route, reached) else {
        return Wire::Unsent(UNREACHED);
    };
    if reached.follows().is_some() {
        return Wire::Unsent(FOLLOWED);
    }
    if reached.feeds().is_some() {
        return Wire::Unsent(FED);
    }
    if reached.pushes().is_some() {
        return Wire::Unsent(PUSHED);
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
