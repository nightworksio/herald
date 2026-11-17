//! The instance as other heralds and fediverse tooling find it: its
//! discovery document with its key set under the sequence the store keeps,
//! its WebFinger account, and NodeInfo (ADR-0044).

use herald_core::{PublishedKeys, itself, media, status};
use herald_keys::{ED25519_LENGTH, ed25519_of};
use herald_peers::discovery::{Instance, NODEINFO_JSON};
use herald_peers::keys::KeySet;
use herald_peers::multibase::ed25519_public;
use herald_ports::{Algorithm, Published, Signer, Store, StoreError, Written};

use super::replies::{Answered, missing, refusal, replied};
use super::{Inbound, Serving};

/// What a host that is not the instance's domain has none of.
const NO_INSTANCE: &str = "herald instance on this host";

/// The instance as its documents describe it, where `host` is its domain.
pub(super) fn instance<'a, S, C, G, V, H, P, K>(
    serving: &Serving<'a, S, C, G, V, H, P, K>,
    host: &str,
) -> Option<Instance<'a>> {
    let domain = serving.config.federation.domain.as_deref()?;
    (domain == host).then(|| Instance {
        domain,
        base: &serving.config.server.base,
        software: itself::NAME,
        version: itself::VERSION,
        repository: herald_core::repository!(),
    })
}

/// The instance's discovery document: its API, its software, and its key
/// set under the sequence the store keeps.
pub(super) async fn discovered<S: Store, C, G: Signer, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    inbound: &Inbound<'_>,
) -> Answered {
    let Some(instance) = instance(serving, inbound.host()) else {
        return missing(NO_INSTANCE);
    };
    let keys = match public_keys(serving).await {
        Ok(keys) => keys,
        Err(why) => return refusal(status::SERVER_ERROR, why),
    };
    let written = keys.iter().map(ed25519_public).collect();
    let attempts = serving.config.attempts.get();
    let sequence = match published(serving.ports.store, written, attempts).await {
        Ok(sequence) => sequence,
        Err(error) => return refusal(status::UNAVAILABLE, error.to_string()),
    };
    let set = KeySet {
        sequence,
        refresh: serving.config.federation.refresh.as_secs().unsigned_abs(),
        keys,
    };
    replied(status::OK, media::JSON, instance.document(&set))
}

/// The instance's Ed25519 public keys, newest first.
async fn public_keys<S, C, G: Signer, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
) -> Result<Vec<[u8; ED25519_LENGTH]>, String> {
    let mut keys = Vec::new();
    for key in &serving.connected.instance {
        let spki = serving
            .ports
            .signer
            .public(Algorithm::Ed25519, key)
            .await
            .map_err(|error| format!("the federation key cannot be read: {error}"))?;
        keys.push(
            ed25519_of(&spki).ok_or_else(|| {
                format!("the federation key {} is not an Ed25519 key", key.name())
            })?,
        );
    }
    Ok(keys)
}

/// The sequence `keys` are published under: the kept one where the store
/// keeps the same keys, else the next, written with compare-and-set within
/// `attempts`; 0 where the store keeps no published key sets.
async fn published<S: Store>(
    store: &S,
    keys: Vec<String>,
    attempts: usize,
) -> Result<u64, StoreError> {
    let Some(kept) = store.published() else {
        return Ok(0);
    };
    for _ in 0..attempts {
        if let Some(sequence) = published_once(kept, &keys).await? {
            return Ok(sequence);
        }
    }
    Err(StoreError::unavailable(format!(
        "the key set was written by another run each of {attempts} times herald wrote it"
    )))
}

/// The sequence `keys` are published under after one read and, where they
/// changed, one write; `None` where another write came first.
async fn published_once<P: Published>(
    kept: &P,
    keys: &[String],
) -> Result<Option<u64>, StoreError> {
    let held = kept.published_keys().await?;
    let written = PublishedKeys::after(held.as_ref().map(|held| &held.value), keys.to_vec());
    if held.as_ref().is_some_and(|held| held.value == written) {
        return Ok(Some(written.sequence));
    }
    match kept
        .put_published_keys(&written, held.map(|held| held.version))
        .await?
    {
        Written::Written(_) => Ok(Some(written.sequence)),
        Written::Conflict => Ok(None),
    }
}

/// Where NodeInfo's document is, on the instance's domain.
pub(super) fn nodeinfo_links<S, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    inbound: &Inbound<'_>,
) -> Answered {
    match instance(serving, inbound.host()) {
        Some(instance) => replied(status::OK, media::JSON, instance.nodeinfo_links()),
        None => missing(NO_INSTANCE),
    }
}

/// The instance's NodeInfo document, counting each route that announces as
/// an actor on its domain.
pub(super) fn nodeinfo<S, C, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    inbound: &Inbound<'_>,
) -> Answered {
    let Some(instance) = instance(serving, inbound.host()) else {
        return missing(NO_INSTANCE);
    };
    let actors = serving
        .connected
        .routes
        .iter()
        .filter_map(|(route, _)| route.reached.federates())
        .filter(|actor| actor.domain() == instance.domain)
        .count();
    replied(status::OK, NODEINFO_JSON, instance.nodeinfo(actors))
}
