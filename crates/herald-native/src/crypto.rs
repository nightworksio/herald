use std::future::{Future, ready};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
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

fn pkcs8(pem: &str) -> Option<Vec<u8>> {
    let body: String = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .map(str::trim)
        .collect();
    STANDARD.decode(body).ok()
}

fn signed(algorithm: Algorithm, key: &Secret, message: &[u8]) -> Option<Vec<u8>> {
    match algorithm {
        Algorithm::HmacSha256 => {
            let key = hmac::Key::new(hmac::HMAC_SHA256, key.expose().as_bytes());
            Some(hmac::sign(&key, message).as_ref().to_vec())
        }
        Algorithm::Ed25519 => {
            let pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&pkcs8(key.expose())?).ok()?;
            Some(pair.sign(message).as_ref().to_vec())
        }
        Algorithm::RsaSha256 => {
            let pair = RsaKeyPair::from_pkcs8(&pkcs8(key.expose())?).ok()?;
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

/// The DER `AlgorithmIdentifier` of `rsaEncryption`, with its null
/// parameters.
const RSA_ENCRYPTION: [u8; 15] = [
    0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00,
];
/// The DER `AlgorithmIdentifier` of Ed25519.
const ED25519_ALGORITHM: [u8; 7] = [0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70];
const SEQUENCE: u8 = 0x30;
const BIT_STRING: u8 = 0x03;

/// `content` as one DER value of `tag`, with its length.
fn der(tag: u8, content: &[u8]) -> Vec<u8> {
    let length = content.len();
    let mut value = vec![tag];
    if length < 0x80 {
        value.push(u8::try_from(length).unwrap_or_default());
    } else {
        let bytes: Vec<u8> = length
            .to_be_bytes()
            .into_iter()
            .skip_while(|byte| *byte == 0)
            .collect();
        value.push(0x80 + u8::try_from(bytes.len()).unwrap_or_default());
        value.extend(bytes);
    }
    value.extend_from_slice(content);
    value
}

/// The `SubjectPublicKeyInfo` of the public key `key` of `algorithm`.
fn subject(algorithm: &[u8], key: &[u8]) -> Vec<u8> {
    let bits = der(BIT_STRING, &[&[0], key].concat());
    der(SEQUENCE, &[algorithm, &bits].concat())
}

fn public_half(algorithm: Algorithm, key: &Secret) -> Option<Vec<u8>> {
    match algorithm {
        Algorithm::HmacSha256 => None,
        Algorithm::Ed25519 => {
            let pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&pkcs8(key.expose())?).ok()?;
            Some(subject(&ED25519_ALGORITHM, pair.public_key().as_ref()))
        }
        Algorithm::RsaSha256 => {
            let pair = RsaKeyPair::from_pkcs8(&pkcs8(key.expose())?).ok()?;
            Some(subject(&RSA_ENCRYPTION, pair.public().as_ref()))
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
