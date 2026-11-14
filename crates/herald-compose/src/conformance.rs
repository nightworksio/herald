use herald_ports::{
    Algorithm, Base, Clock as _, Outbound, SecretError, Secrets as _, Signer as _, Verifier as _,
};
use herald_testing::Failure;
use jiff::Timestamp;

use crate::Host;

/// The variable a host is given set to [`CONFORMS`] for the host suite.
pub const CONFORMANCE_VAR: &str = "HERALD_CONFORMANCE";

/// The variable a host is given set empty for the host suite.
pub const CONFORMANCE_EMPTY: &str = "HERALD_CONFORMANCE_EMPTY";

/// The secret a host holds set to [`CONFORMS`] for the host suite.
pub const CONFORMANCE_SECRET: &str = "HERALD_CONFORMANCE_SECRET";

/// What the host suite's variable and secret hold.
pub const CONFORMS: &str = "conforms";

/// A name no host gives the host suite, as a variable or a secret.
const UNSET: &str = "HERALD_CONFORMANCE_UNSET";

/// When the host suite was written, 2026-11-01: no host's clock is behind it.
const WRITTEN: Timestamp = Timestamp::constant(1_793_491_200, 0);

/// What the host suite signs.
const MESSAGE: &[u8] = b"herald conforms";

/// A base every host reaches.
const BASE: &str = "https://api.example";

/// Whether `host`, given [`CONFORMANCE_VAR`] and [`CONFORMANCE_SECRET`] set
/// to [`CONFORMS`] and [`CONFORMANCE_EMPTY`] set empty, reads a variable
/// only where it is set and not empty and a secret only where it is held,
/// gives a new nonce each time and the time as it is, verifies what it signs
/// for that message alone, and reaches an `https` base. Every host passes
/// this suite, and so does the fake its tests compose over.
///
/// # Errors
///
/// The first way the host fails, as a sentence.
pub async fn host_conformance<H: Host>(host: &H) -> Result<(), Failure> {
    let secret = host.secrets().secret(CONFORMANCE_SECRET).await;
    let unset = host.secrets().secret(UNSET).await;
    let verified = match &secret {
        Ok(secret) => match host
            .signer()
            .sign(Algorithm::HmacSha256, secret, MESSAGE)
            .await
        {
            Ok(signature) => {
                let key = secret.expose().as_bytes();
                let verify = |message| {
                    host.verifier()
                        .verify(Algorithm::HmacSha256, key, message, &signature)
                };
                verify(MESSAGE).await && !verify(b"herald conforms not").await
            }
            Err(_) => false,
        },
        Err(_) => false,
    };
    let nonces = [host.nonce(), host.nonce()];
    let checks = [
        (
            host.var(CONFORMANCE_VAR) == Some(CONFORMS),
            "a variable the platform sets was not read as set",
        ),
        (
            host.var(CONFORMANCE_EMPTY).is_none(),
            "a variable set empty was read as set",
        ),
        (
            host.var(UNSET).is_none(),
            "a variable the platform does not set was read",
        ),
        (
            secret.is_ok_and(|secret| {
                secret.name() == CONFORMANCE_SECRET && secret.expose() == CONFORMS
            }),
            "a secret the platform holds was not read as held",
        ),
        (
            unset
                == Err(SecretError::Missing {
                    name: UNSET.to_owned(),
                }),
            "a secret the platform does not hold was not refused as missing",
        ),
        (
            !nonces[0].is_empty() && nonces[0] != nonces[1],
            "two nonces were alike",
        ),
        (
            host.clock().now() >= WRITTEN,
            "the clock is behind the time the host suite was written",
        ),
        (
            verified,
            "a signature the host made was not verified for its message alone",
        ),
        (
            host.connect(Base::At(BASE), &Outbound::default()).is_ok(),
            "an https base could not be reached",
        ),
    ];
    checks
        .into_iter()
        .find(|(held, _)| !held)
        .map_or(Ok(()), |(_, failure)| Err(Failure(failure.to_owned())))
}

#[cfg(test)]
mod tests;
