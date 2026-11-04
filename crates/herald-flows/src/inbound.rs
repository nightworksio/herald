use std::fmt;

use herald_core::{Addressed, Claim, Refusal, Scheme, Signature, activity_claim, claim, key, same};
use herald_dest_fediverse::{
    ActorError, ActorKey, Found, activity_json, actor_key, actor_of, confirms, key_request,
    same_origin,
};
use herald_ports::{Admitted, Algorithm, Http, Store, StoreError, Verifier};
use jiff::Timestamp;

/// Why an inbound request is not let through to a parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejected {
    /// It is refused before its signature is checked.
    Refused(Refusal),
    /// Its signature is not its sender's.
    Forged,
    /// The key it would be checked with is not written in its form.
    Unkeyed,
    /// It was delivered before, and is taken for a replay (ADR-0013).
    Replayed,
    /// The store could not say whether it was delivered before, so it is
    /// not let through.
    Unadmitted {
        /// Why the store could not answer.
        reason: String,
    },
    /// The key its signature names could not be read from that key's
    /// document.
    Unfetched {
        /// Why not.
        reason: String,
    },
    /// It names an actor other than the owner of the key that signed it, or
    /// has its own id on another origin than that actor.
    Impersonated,
}

/// A request [`verified`] let through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// What it is delivered as, as its signature covers it, where its scheme
    /// names its deliveries.
    pub delivery: Option<String>,
    /// Who signed it, where its scheme names a signer: the actor owning the
    /// key an ActivityPub activity is signed with, which is the activity's
    /// own actor.
    pub signer: Option<String>,
}

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(refusal) => refusal.fmt(f),
            Self::Forged => f.write_str("the request's signature is not its sender's"),
            Self::Unkeyed => f.write_str(
                "the key the request is checked with is not written in its form, so no request is let through",
            ),
            Self::Replayed => f.write_str("the request was delivered before, so it is taken for a replay"),
            Self::Unadmitted { reason } => write!(
                f,
                "the store could not say whether the request was delivered before, so it is not let through: {reason}"
            ),
            Self::Unfetched { reason } => write!(
                f,
                "the key the activity is signed with could not be read: {reason}"
            ),
            Self::Impersonated => f.write_str(
                "the activity names an actor other than the owner of the key that signed it, or has its id on another origin",
            ),
        }
    }
}

/// Whether a request of `scheme`, with its `headers` and raw `body`, is
/// signed by the holder of `written` at `now`: a shared secret for an HMAC,
/// the sender's public key for Ed25519, each as [`key`] reads it
/// (ADR-0007). Only a request this lets through may reach a parser. An
/// ActivityPub activity is checked by [`activity_verified`], which reads
/// the key its signature names and holds its actor to that key's owner.
///
/// # Errors
///
/// Why the request is rejected.
pub async fn verified<V: Verifier>(
    verifier: &V,
    scheme: Scheme,
    written: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    now: Timestamp,
) -> Result<Verified, Rejected> {
    let key = key(scheme, written)
        .filter(|_| scheme != Scheme::ActivityPub)
        .ok_or(Rejected::Unkeyed)?;
    let claim = claim(scheme, headers, body, now).map_err(Rejected::Refused)?;
    signed(verifier, &key, &claim).await?;
    Ok(Verified {
        delivery: claim.delivery,
        signer: None,
    })
}

/// Whether an ActivityPub activity sent to `addressed`, with its `headers`
/// and raw `body`, is signed at `now` by the key its signature names, served
/// as ActivityPub JSON read from that key's
/// document through `http`, which reaches only public addresses, and
/// names that key's owner as its actor, with its own id, where it has one,
/// on that actor's origin (ADR-0007). A key whose own
/// document names its owner is that owner's only where the owner's own
/// document lists it. Only an activity this
/// lets through may reach a parser.
///
/// # Errors
///
/// Why the activity is rejected.
pub async fn activity_verified<V: Verifier, H: Http>(
    verifier: &V,
    http: &H,
    addressed: Addressed<'_>,
    headers: &[(&str, &str)],
    body: &[u8],
    now: Timestamp,
) -> Result<Verified, Rejected> {
    let claim = activity_claim(addressed, headers, body, now).map_err(Rejected::Refused)?;
    let named = claim.signer.as_deref().unwrap_or_default();
    let unfetched = |reason: String| Rejected::Unfetched { reason };
    let found = actor_key(&fetched(http, named).await.map_err(unfetched)?, named)
        .map_err(|error| unfetched(error.to_string()))?;
    let owned = match found {
        Found::Held(owned) => owned,
        Found::Claimed { owner, pem } => {
            if !confirms(
                &fetched(http, &owner).await.map_err(unfetched)?,
                &owner,
                named,
            ) {
                return Err(unfetched(ActorError::Unconfirmed.to_string()));
            }
            ActorKey { actor: owner, pem }
        }
    };
    let key = key(Scheme::ActivityPub, &owned.pem).ok_or(Rejected::Unkeyed)?;
    signed(verifier, &key, &claim).await?;
    let Some((actor, own)) = actor_of(body) else {
        return Err(Rejected::Impersonated);
    };
    if actor != owned.actor || own.is_some_and(|own| !same_origin(&own, &actor)) {
        return Err(Rejected::Impersonated);
    }
    Ok(Verified {
        delivery: claim.delivery,
        signer: Some(owned.actor),
    })
}

/// The document `id` names, read through `http`.
pub(crate) async fn fetched<H: Http>(http: &H, id: &str) -> Result<Vec<u8>, String> {
    let request =
        key_request(id).ok_or_else(|| format!("{id} is not an HTTPS URL naming a host"))?;
    let response = http
        .send(request)
        .await
        .map_err(|error| error.to_string())?;
    if !response.succeeded() {
        return Err(format!("its server answered {}", response.status));
    }
    if !activity_json(response.header("content-type")) {
        return Err("its server did not answer it as ActivityPub JSON".to_owned());
    }
    Ok(response.body)
}

/// Whether one of `claim`'s signatures holds under `key`.
async fn signed<V: Verifier>(verifier: &V, key: &[u8], claim: &Claim) -> Result<(), Rejected> {
    let algorithm = match claim.kind {
        Signature::HmacSha256 => Algorithm::HmacSha256,
        Signature::Ed25519 => Algorithm::Ed25519,
        Signature::RsaSha256 => Algorithm::RsaSha256,
        Signature::Token => {
            return claim
                .signatures
                .iter()
                .any(|token| same(key, token))
                .then_some(())
                .ok_or(Rejected::Forged);
        }
    };
    for signature in &claim.signatures {
        if verifier
            .verify(algorithm, key, &claim.message, signature)
            .await
        {
            return Ok(());
        }
    }
    Err(Rejected::Forged)
}

/// Admits a request `verified` let through, by the delivery it names, at
/// `now` (ADR-0013). A request whose scheme names no delivery, or a store
/// that keeps none, admits it, and the ledger skips what a replay would
/// announce.
///
/// # Errors
///
/// [`Rejected::Replayed`] for a delivery admitted before, and
/// [`Rejected::Unadmitted`] where the store cannot answer.
pub async fn admitted<S: Store>(
    store: &S,
    scheme: Scheme,
    verified: &Verified,
    now: Timestamp,
) -> Result<(), Rejected> {
    let Some(delivery) = &verified.delivery else {
        return Ok(());
    };
    admission(store.admit(scheme, delivery, now).await)
}

/// What a store's answer to admitting a delivery lets through.
fn admission(answer: Result<Admitted, StoreError>) -> Result<(), Rejected> {
    match answer {
        Ok(Admitted::Fresh) | Err(StoreError::Unsupported { .. }) => Ok(()),
        Ok(Admitted::Seen) => Err(Rejected::Replayed),
        Err(error) => Err(Rejected::Unadmitted {
            reason: error.to_string(),
        }),
    }
}

/// Forgets the delivery of a request [`admitted`] let through whose
/// processing failed, so the sender's next delivery of it is admitted
/// (ADR-0013).
///
/// # Errors
///
/// Where the store cannot forget it.
pub async fn released<S: Store>(
    store: &S,
    scheme: Scheme,
    verified: &Verified,
) -> Result<(), StoreError> {
    let Some(delivery) = &verified.delivery else {
        return Ok(());
    };
    forgotten(store.release(scheme, delivery).await)
}

/// What a store's answer to forgetting a delivery leaves to report: a store
/// that keeps no deliveries has nothing to forget.
fn forgotten(answer: Result<(), StoreError>) -> Result<(), StoreError> {
    match answer {
        Err(StoreError::Unsupported { .. }) => Ok(()),
        answer => answer,
    }
}
