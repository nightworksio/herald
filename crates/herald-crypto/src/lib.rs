//! herald's signing and verifying on ring (ADR-0007), for each driver that
//! compiles it: the native binary, the WASI component and Fastly Compute.

use std::future::{Future, ready};

use herald_keys::{Public, pem, subject};
use herald_ports::{Algorithm, CryptoError, Secret, Signer, Verifier};
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

fn signed(algorithm: Algorithm, key: &Secret, message: &[u8]) -> Option<Vec<u8>> {
    match algorithm {
        Algorithm::HmacSha256 => {
            let key = hmac::Key::new(hmac::HMAC_SHA256, key.expose().as_bytes());
            Some(hmac::sign(&key, message).as_ref().to_vec())
        }
        Algorithm::Ed25519 => {
            let pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&pem(key.expose())?).ok()?;
            Some(pair.sign(message).as_ref().to_vec())
        }
        Algorithm::RsaSha256 => {
            let pair = RsaKeyPair::from_pkcs8(&pem(key.expose())?).ok()?;
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
            let pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&pem(key.expose())?).ok()?;
            Some(subject(Public::Ed25519, pair.public_key().as_ref()))
        }
        Algorithm::RsaSha256 => {
            let pair = RsaKeyPair::from_pkcs8(&pem(key.expose())?).ok()?;
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
