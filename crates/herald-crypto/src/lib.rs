//! herald's signing and verifying on ring (ADR-0007), for each driver that
//! compiles it: the native binary, the WASI component and Fastly Compute;
//! and, where the `keygen` feature is on, as the native CLI builds it, new
//! RSA keys on AWS-LC.

#[cfg(feature = "keygen")]
pub mod keygen;

use std::collections::BTreeMap;
use std::future::{Future, ready};
use std::sync::{Arc, Mutex, PoisonError};

use herald_keys::{Public, private_key, subject};
use herald_ports::{Algorithm, CryptoError, Sealing, Secret, Signer, Verifier};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::hkdf::{HKDF_SHA256, Salt};
use ring::hmac;
use ring::rand::{SecureRandom as _, SystemRandom};
use ring::signature::{
    ED25519, Ed25519KeyPair, KeyPair as _, RSA_PKCS1_2048_8192_SHA256, RSA_PKCS1_SHA256,
    RsaKeyPair, UnparsedPublicKey,
};
use zeroize::Zeroizing;

/// Signs with ring: HMAC under the secret's bytes, and Ed25519 or RSA under a
/// PKCS#8 private key the secret holds as PEM. Each RSA key pair it parses is
/// kept by the name the config gives its secret, until the last clone of the
/// signer is dropped.
#[derive(Debug, Clone, Default)]
pub struct RingSigner {
    pairs: Arc<Mutex<BTreeMap<String, Arc<RsaKeyPair>>>>,
}

/// Verifies with ring: HMAC under the shared secret, Ed25519 under its
/// 32-byte public key, and RSA under its PKCS#1 public key.
#[derive(Debug, Clone, Copy, Default)]
pub struct RingVerifier;

fn pkcs8(key: &Secret) -> Option<Zeroizing<Vec<u8>>> {
    private_key(key.expose()).map(Zeroizing::new)
}

impl RingSigner {
    fn rsa_pair(&self, key: &Secret) -> Option<Arc<RsaKeyPair>> {
        let mut pairs = self.pairs.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(pair) = pairs.get(key.name()) {
            return Some(Arc::clone(pair));
        }
        let pair = Arc::new(RsaKeyPair::from_pkcs8(&pkcs8(key)?).ok()?);
        pairs.insert(key.name().to_owned(), Arc::clone(&pair));
        Some(pair)
    }

    fn signed(&self, algorithm: Algorithm, key: &Secret, message: &[u8]) -> Option<Vec<u8>> {
        match algorithm {
            Algorithm::HmacSha256 => {
                let key = hmac::Key::new(hmac::HMAC_SHA256, key.expose().as_bytes());
                Some(hmac::sign(&key, message).as_ref().to_vec())
            }
            Algorithm::Ed25519 => {
                let pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&pkcs8(key)?).ok()?;
                Some(pair.sign(message).as_ref().to_vec())
            }
            Algorithm::RsaSha256 => {
                let pair = self.rsa_pair(key)?;
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

    fn public_half(&self, algorithm: Algorithm, key: &Secret) -> Option<Vec<u8>> {
        match algorithm {
            Algorithm::HmacSha256 => None,
            Algorithm::Ed25519 => {
                let pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&pkcs8(key)?).ok()?;
                Some(subject(Public::Ed25519, pair.public_key().as_ref()))
            }
            Algorithm::RsaSha256 => {
                let pair = self.rsa_pair(key)?;
                Some(subject(Public::Rsa, pair.public().as_ref()))
            }
        }
    }
}

/// The AES-256-GCM key HKDF-SHA256 derives from `key` for `purpose`.
fn sealing(key: &Secret, purpose: &str) -> Option<LessSafeKey> {
    let extracted = Salt::new(HKDF_SHA256, &[]).extract(key.expose().as_bytes());
    let info = [purpose.as_bytes()];
    let derived = extracted.expand(&info, &AES_256_GCM).ok()?;
    Some(LessSafeKey::new(UnboundKey::from(derived)))
}

/// `plain` sealed under `key` as `with` says, a fresh nonce first.
fn sealed(key: &Secret, with: Sealing<'_>, plain: &[u8]) -> Option<Vec<u8>> {
    let sealing = sealing(key, with.purpose)?;
    let mut nonce = [0; NONCE_LEN];
    SystemRandom::new().fill(&mut nonce).ok()?;
    let mut held = plain.to_vec();
    sealing
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(with.bound),
            &mut held,
        )
        .ok()?;
    Some([&nonce[..], &held].concat())
}

/// What `sealed` holds, sealed under `key` as `with` says.
fn opened(key: &Secret, with: Sealing<'_>, sealed: &[u8]) -> Option<Vec<u8>> {
    let sealing = sealing(key, with.purpose)?;
    let (nonce, held) = sealed.split_at_checked(NONCE_LEN)?;
    let nonce = Nonce::try_assume_unique_for_key(nonce).ok()?;
    let mut held = held.to_vec();
    let plain = sealing
        .open_in_place(nonce, Aad::from(with.bound), &mut held)
        .ok()?;
    Some(plain.to_vec())
}

impl Signer for RingSigner {
    fn sign(
        &self,
        algorithm: Algorithm,
        key: &Secret,
        message: &[u8],
    ) -> impl Future<Output = Result<Vec<u8>, CryptoError>> + Send {
        ready(
            self.signed(algorithm, key, message)
                .ok_or_else(|| CryptoError::BadKey {
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
            self.public_half(algorithm, key)
                .ok_or_else(|| CryptoError::BadKey {
                    name: key.name().to_owned(),
                }),
        )
    }

    fn seal(
        &self,
        key: &Secret,
        sealing: Sealing<'_>,
        plain: &[u8],
    ) -> impl Future<Output = Result<Vec<u8>, CryptoError>> + Send {
        ready(sealed(key, sealing, plain).ok_or(CryptoError::Unsealable {
            name: key.name().to_owned(),
        }))
    }

    fn open(
        &self,
        key: &Secret,
        sealing: Sealing<'_>,
        sealed: &[u8],
    ) -> impl Future<Output = Result<Vec<u8>, CryptoError>> + Send {
        ready(
            opened(key, sealing, sealed).ok_or_else(|| CryptoError::Unsealed {
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
