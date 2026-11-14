//! herald's signing and verifying on ring (ADR-0007), for each driver that
//! compiles it: the native binary, the WASI component and Fastly Compute.

use std::collections::BTreeMap;
use std::future::{Future, ready};
use std::sync::{Arc, Mutex, PoisonError};

use herald_keys::{PRIVATE_KEY, Public, pem, subject};
use herald_ports::{Algorithm, CryptoError, Secret, Signer, Verifier};
use ring::digest::{SHA256, digest};
use ring::hmac;
use ring::rand::SystemRandom;
use ring::signature::{
    ED25519, Ed25519KeyPair, KeyPair as _, RSA_PKCS1_2048_8192_SHA256, RSA_PKCS1_SHA256,
    RsaKeyPair, UnparsedPublicKey,
};

/// Signs with ring: HMAC under the secret's bytes, and Ed25519 or RSA under a
/// PKCS#8 private key the secret holds as PEM.
#[derive(Debug, Clone, Copy, Default)]
pub struct RingSigner;

/// Verifies with ring: HMAC under the shared secret, Ed25519 under its
/// 32-byte public key, and RSA under its PKCS#1 public key.
#[derive(Debug, Clone, Copy, Default)]
pub struct RingVerifier;

/// Each RSA key pair parsed, by the SHA-256 of the PEM it was parsed from,
/// so a key that signs many deliveries is parsed once.
static RSA_PAIRS: Mutex<BTreeMap<[u8; 32], Arc<RsaKeyPair>>> = Mutex::new(BTreeMap::new());

/// The RSA key pair the PEM `key` holds, parsed once per process.
fn rsa_pair(key: &Secret) -> Option<Arc<RsaKeyPair>> {
    let pem_digest: [u8; 32] = digest(&SHA256, key.expose().as_bytes())
        .as_ref()
        .try_into()
        .ok()?;
    let mut pairs = RSA_PAIRS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(pair) = pairs.get(&pem_digest) {
        return Some(Arc::clone(pair));
    }
    let pair = Arc::new(RsaKeyPair::from_pkcs8(&pem(key.expose(), PRIVATE_KEY)?).ok()?);
    pairs.insert(pem_digest, Arc::clone(&pair));
    Some(pair)
}

fn signed(algorithm: Algorithm, key: &Secret, message: &[u8]) -> Option<Vec<u8>> {
    match algorithm {
        Algorithm::HmacSha256 => {
            let key = hmac::Key::new(hmac::HMAC_SHA256, key.expose().as_bytes());
            Some(hmac::sign(&key, message).as_ref().to_vec())
        }
        Algorithm::Ed25519 => {
            let pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&pem(key.expose(), PRIVATE_KEY)?)
                .ok()?;
            Some(pair.sign(message).as_ref().to_vec())
        }
        Algorithm::RsaSha256 => {
            let pair = rsa_pair(key)?;
            let mut signature = vec![0; pair.public().modulus_len()];
            pair.sign(
                &RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                message,
                &mut signature,
            )
            .ok()?;
            Some(signature)
        }
    }
}

fn public_half(algorithm: Algorithm, key: &Secret) -> Option<Vec<u8>> {
    match algorithm {
        Algorithm::HmacSha256 => None,
        Algorithm::Ed25519 => {
            let pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&pem(key.expose(), PRIVATE_KEY)?)
                .ok()?;
            Some(subject(Public::Ed25519, pair.public_key().as_ref()))
        }
        Algorithm::RsaSha256 => {
            let pair = rsa_pair(key)?;
            Some(subject(Public::Rsa, pair.public().as_ref()))
        }
    }
}

impl Signer for RingSigner {
    fn sign(
        &self,
        algorithm: Algorithm,
        key: &Secret,
        message: &[u8],
    ) -> impl Future<Output = Result<Vec<u8>, CryptoError>> + Send {
        ready(
            signed(algorithm, key, message).ok_or_else(|| CryptoError::BadKey {
                name: key.name().to_owned(),
            }),
        )
    }

    fn public(
        &self,
        algorithm: Algorithm,
        key: &Secret,
    ) -> impl Future<Output = Result<Vec<u8>, CryptoError>> + Send {
        ready(
            public_half(algorithm, key).ok_or_else(|| CryptoError::BadKey {
                name: key.name().to_owned(),
            }),
        )
    }
}

impl Verifier for RingVerifier {
    fn verify(
        &self,
        algorithm: Algorithm,
        key: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> impl Future<Output = bool> + Send {
        ready(match algorithm {
            Algorithm::HmacSha256 => {
                hmac::verify(&hmac::Key::new(hmac::HMAC_SHA256, key), message, signature).is_ok()
            }
            Algorithm::Ed25519 => UnparsedPublicKey::new(&ED25519, key)
                .verify(message, signature)
                .is_ok(),
            Algorithm::RsaSha256 => UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, key)
                .verify(message, signature)
                .is_ok(),
        })
    }
}

#[cfg(test)]
mod tests;
