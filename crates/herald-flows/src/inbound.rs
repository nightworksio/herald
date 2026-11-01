use std::fmt;

use herald_core::{Refusal, Scheme, Signature, claim, key, same};
use herald_ports::{Admitted, Algorithm, Store, StoreError, Verifier};
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
}

/// A request [`verified`] let through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// What it is delivered as, as its signature covers it, where its scheme
    /// names its deliveries.
    pub delivery: Option<String>,
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
        }
    }
}

/// Whether a request of `scheme`, with its `headers` and raw `body`, is
/// signed by the holder of `written` at `now`: a shared secret for an HMAC,
/// the sender's public key for Ed25519, each as [`key`] reads it
/// (ADR-0007). Only a request this lets through may reach a parser.
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
    let key = key(scheme, written).ok_or(Rejected::Unkeyed)?;
    let claim = claim(scheme, headers, body, now).map_err(Rejected::Refused)?;
    let algorithm = match claim.kind {
        Signature::HmacSha256 => Algorithm::HmacSha256,
        Signature::Ed25519 => Algorithm::Ed25519,
        Signature::Token => {
            return if claim.signatures.iter().any(|token| same(&key, token)) {
                Ok(Verified {
                    delivery: claim.delivery,
                })
            } else {
                Err(Rejected::Forged)
            };
        }
    };
    for signature in &claim.signatures {
        if verifier
            .verify(algorithm, &key, &claim.message, signature)
            .await
        {
            return Ok(Verified {
                delivery: claim.delivery,
            });
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
    match store.admit(scheme, delivery, now).await {
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
    match &verified.delivery {
        Some(delivery) => match store.release(scheme, delivery).await {
            Err(StoreError::Unsupported { .. }) => Ok(()),
            released => released,
        },
        None => Ok(()),
    }
}
