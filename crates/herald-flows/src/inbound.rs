use std::fmt;

use herald_core::{Refusal, Scheme, Signature, claim, key, same};
use herald_ports::{Algorithm, Verifier};
use jiff::Timestamp;

/// Why an inbound request is not let through to a parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejected {
    /// It is refused before its signature is checked.
    Refused(Refusal),
    /// Its signature is not its sender's.
    Forged,
    /// The key it would be checked with is not written in its scheme's form.
    Unkeyed,
}

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(refusal) => refusal.fmt(f),
            Self::Forged => f.write_str("the request's signature is not its sender's"),
            Self::Unkeyed => f.write_str(
                "the key the request is checked with is not written in its form, so no request is let through",
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
) -> Result<(), Rejected> {
    let key = key(scheme, written).ok_or(Rejected::Unkeyed)?;
    let claim = claim(scheme, headers, body, now).map_err(Rejected::Refused)?;
    let algorithm = match claim.kind {
        Signature::HmacSha256 => Algorithm::HmacSha256,
        Signature::Ed25519 => Algorithm::Ed25519,
        Signature::Token => {
            return if claim.signatures.iter().any(|token| same(&key, token)) {
                Ok(())
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
            return Ok(());
        }
    }
    Err(Rejected::Forged)
}
