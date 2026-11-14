//! Inbound requests verified as their senders' and admitted once before a
//! parser reads them (ADR-0007, ADR-0013).

use std::fmt;

use herald_core::Signed;
use herald_core::{
    Addressed, Claim, Inbound, Refusal, Scheme, Signature, activity_claim, claim, key, same,
};
use herald_dest_fediverse::{
    ActorError, ActorKey, KeyOwner, activity_json, actor_key, actor_of, confirms, key_request,
    same_origin,
};
use herald_ports::{Admitted, Deliveries, Http, Keyring, KnownKey, Store, StoreError, Verifier};
use jiff::{SignedDuration, Timestamp};

/// When an inbound request is checked, and the bounds it is held to.
#[derive(Debug, Clone, Copy)]
pub struct Checking<'a> {
    /// The time it is checked at.
    pub now: Timestamp,
    /// What it may carry and still be checked.
    pub bounds: &'a Inbound,
}

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
    /// The keys its signature is checked with could not be read or kept, so
    /// it is not let through.
    Unkept {
        /// Why not.
        reason: String,
    },
}

/// Where the keys activities were checked with are kept, and how long one
/// is used before its document is read again.
#[derive(Debug)]
pub struct Keys<'a, K> {
    /// Where they are kept.
    pub keyring: &'a K,
    /// How long one is used before its document is read again.
    pub refreshed: SignedDuration,
    /// How long after one is read an activity that does not hold under it
    /// is refused rather than having it read again.
    pub reread: SignedDuration,
}

impl<K: Keyring> Keys<'_, K> {
    /// Every key read longer ago than its refresh at `now`, forgotten.
    ///
    /// # Errors
    ///
    /// When the keys could not be forgotten.
    pub async fn sweep(&self, now: Timestamp) -> Result<(), StoreError> {
        let before = now.checked_sub(self.refreshed).unwrap_or(Timestamp::MIN);
        kept_nowhere(self.keyring.forget_before(before).await)
    }
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
            Self::Unkept { reason } => write!(
                f,
                "the keys activities are checked with could not be read or kept, so the activity is not let through: {reason}"
            ),
        }
    }
}

/// Whether a request of `scheme`, with its `headers` and raw `body`, is
/// signed by the holder of `written` as `checking` holds it: a shared secret for an HMAC,
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
    checking: Checking<'_>,
) -> Result<Verified, Rejected> {
    let key = key(scheme, written)
        .filter(|_| scheme.signed != Signed::HttpSignature)
        .ok_or(Rejected::Unkeyed)?;
    let claim =
        claim(scheme, headers, body, checking.now, checking.bounds).map_err(Rejected::Refused)?;
    signed(verifier, &key, &claim).await?;
    Ok(Verified {
        delivery: claim.delivery,
        signer: None,
    })
}

/// Whether an ActivityPub activity sent to `addressed`, with its `headers`
/// and raw `body`, is signed as `checking` holds it by the key its signature names, served
/// as ActivityPub JSON read from that key's
/// document through `http`, which reaches only public addresses, and
/// names that key's owner as its actor, with its own id, where it has one,
/// on that actor's origin (ADR-0007). A key whose own
/// document names its owner is that owner's only where the owner's own
/// document lists it. A key `keys` keeps, read within its refresh, is used
/// without reading its document again; where the activity does not hold
/// under it, its document is read again, as a key rotated since, unless it
/// was read within `keys.reread`, when the activity is refused. Only an
/// activity this lets through may reach a parser.
///
/// # Errors
///
/// Why the activity is rejected.
pub async fn activity_verified<V: Verifier, H: Http, K: Keyring>(
    verifier: &V,
    http: &H,
    keys: &Keys<'_, K>,
    addressed: Addressed<'_>,
    headers: &[(&str, &str)],
    body: &[u8],
    checking: Checking<'_>,
) -> Result<Verified, Rejected> {
    let now = checking.now;
    let claim = activity_claim(addressed, headers, body, now, checking.bounds)
        .map_err(Rejected::Refused)?;
    let named = claim.signer.as_deref().unwrap_or_default();
    let unkept = |error: StoreError| Rejected::Unkept {
        reason: error.to_string(),
    };
    let kept = kept_nowhere(keys.keyring.known(named).await)
        .map_err(unkept)?
        .filter(|known| {
            known
                .fetched
                .checked_add(keys.refreshed)
                .is_ok_and(|stale| stale > now)
        });
    if let Some(known) = kept {
        let recent = known
            .fetched
            .checked_add(keys.reread)
            .is_ok_and(|after| after > now);
        let owned = ActorKey {
            actor: known.owner,
            pem: known.pem,
        };
        let checked = owned_by(verifier, &claim, owned, body).await;
        if checked.is_ok() || recent {
            return checked;
        }
    }
    let owned = read_key(http, named, checking.bounds).await?;
    let known = KnownKey {
        owner: owned.actor.clone(),
        pem: owned.pem.clone(),
        fetched: now,
    };
    kept_nowhere(keys.keyring.know(named, &known).await).map_err(unkept)?;
    owned_by(verifier, &claim, owned, body).await
}

/// The key `named` names, read from its document through `http`, and its
/// owner, confirmed by the owner's own document where the key's names it.
async fn read_key<H: Http>(http: &H, named: &str, bounds: &Inbound) -> Result<ActorKey, Rejected> {
    let unfetched = |reason: String| Rejected::Unfetched { reason };
    let found = actor_key(
        &fetched(http, named, bounds).await.map_err(unfetched)?,
        named,
    )
    .map_err(|error| unfetched(error.to_string()))?;
    match found {
        KeyOwner::Held(owned) => Ok(owned),
        KeyOwner::Claimed { owner, pem } => {
            if !confirms(
                &fetched(http, &owner, bounds).await.map_err(unfetched)?,
                &owner,
                named,
            ) {
                return Err(unfetched(ActorError::Unconfirmed.to_string()));
            }
            Ok(ActorKey { actor: owner, pem })
        }
    }
}

/// Whether `claim` holds under `owned`, and the activity `body` names that
/// key's owner as its actor, with its own id on that actor's origin.
async fn owned_by<V: Verifier>(
    verifier: &V,
    claim: &Claim,
    owned: ActorKey,
    body: &[u8],
) -> Result<Verified, Rejected> {
    let key = key(herald_dest_fediverse::ACTOR_SCHEME, &owned.pem).ok_or(Rejected::Unkeyed)?;
    signed(verifier, &key, claim).await?;
    let Some((actor, own)) = actor_of(body) else {
        return Err(Rejected::Impersonated);
    };
    if actor != owned.actor || own.is_some_and(|own| !same_origin(&own, &actor)) {
        return Err(Rejected::Impersonated);
    }
    Ok(Verified {
        delivery: claim.delivery.clone(),
        signer: Some(owned.actor),
    })
}

/// The document `id` names, read through `http`, no further than `bounds`
/// let a document be read.
pub(crate) async fn fetched<H: Http>(
    http: &H,
    id: &str,
    bounds: &Inbound,
) -> Result<Vec<u8>, String> {
    let request = key_request(id, bounds.document)
        .ok_or_else(|| format!("{id} is not an HTTPS URL naming a host"))?;
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
        Signature::Signed(algorithm) => algorithm,
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
    admitting: Admission,
    scheme: Scheme,
    verified: &Verified,
    now: Timestamp,
) -> Result<(), Rejected> {
    let Some(delivery) = &verified.delivery else {
        return Ok(());
    };
    admission(
        store.deliveries().admit(scheme, delivery, now).await,
        admitting,
    )
}

/// Whether a request's delivery is let through once, or again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Admission {
    /// Once: a delivery seen before is a replay, refused.
    #[default]
    Once,
    /// Again: the request is one a driver answered and handed itself again
    /// to work, as the Lambda does, so a delivery seen before is let
    /// through.
    Again,
}

/// What a store's answer to admitting a delivery lets through.
fn admission(answer: Result<Admitted, StoreError>, admitting: Admission) -> Result<(), Rejected> {
    match answer {
        Ok(Admitted::Fresh) | Err(StoreError::Unsupported { .. }) => Ok(()),
        Ok(Admitted::Seen) if admitting == Admission::Again => Ok(()),
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
    kept_nowhere(store.deliveries().release(scheme, delivery).await)
}

/// What a store's `answer` leaves to report: a store that keeps none of
/// what it was asked for has nothing to forget, keep or find.
fn kept_nowhere<T: Default>(answer: Result<T, StoreError>) -> Result<T, StoreError> {
    match answer {
        Err(StoreError::Unsupported { .. }) => Ok(T::default()),
        answer => answer,
    }
}
