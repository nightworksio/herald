use std::fmt;
use std::future::Future;

use herald_core::Algorithm;

use crate::secret::Secret;

/// Why a signature could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CryptoError {
    /// The key is not a key for the algorithm.
    BadKey {
        /// The secret's name.
        name: String,
    },
    /// Nothing can be sealed under a key derived from the secret on this
    /// host.
    Unsealable {
        /// The secret's name.
        name: String,
    },
    /// What was sealed under a key derived from the secret could not be
    /// opened: it was sealed under another key, or changed since.
    Unsealed {
        /// The secret's name.
        name: String,
    },
}

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadKey { name } => write!(
                f,
                "the secret {name} is not a key for this signature; give a private key as PEM, PKCS#8 or an RSA key's PKCS#1"
            ),
            Self::Unsealable { name } => {
                write!(
                    f,
                    "nothing can be sealed under the secret {name} on this host"
                )
            }
            Self::Unsealed { name } => write!(
                f,
                "what was sealed under the secret {name} cannot be opened: it was sealed under another key, or changed since"
            ),
        }
    }
}

/// Signs with a key it is handed as a secret.
pub trait Signer: Sync {
    /// The signature of `message` by `key` under `algorithm`.
    fn sign(
        &self,
        algorithm: Algorithm,
        key: &Secret,
        message: &[u8],
    ) -> impl Future<Output = Result<Vec<u8>, CryptoError>> + Send;

    /// The public half of `key` under `algorithm`, as a DER
    /// `SubjectPublicKeyInfo`: what an ActivityPub actor publishes as its
    /// `publicKeyPem`.
    fn public(
        &self,
        algorithm: Algorithm,
        key: &Secret,
    ) -> impl Future<Output = Result<Vec<u8>, CryptoError>> + Send;

    /// `plain` sealed with AES-256-GCM under a key derived from `key` by
    /// HKDF-SHA256 for the purpose `sealing` names, bound to its data, its
    /// nonce first; a host that seals nothing refuses.
    fn seal(
        &self,
        key: &Secret,
        _sealing: Sealing<'_>,
        _plain: &[u8],
    ) -> impl Future<Output = Result<Vec<u8>, CryptoError>> + Send {
        std::future::ready(Err(CryptoError::Unsealable {
            name: key.name().to_owned(),
        }))
    }

    /// What [`Signer::seal`] sealed under `key` as `sealing` says as
    /// `sealed`, opened; refused where it was sealed under another key,
    /// purpose or bound data, or changed since.
    fn open(
        &self,
        key: &Secret,
        _sealing: Sealing<'_>,
        _sealed: &[u8],
    ) -> impl Future<Output = Result<Vec<u8>, CryptoError>> + Send {
        std::future::ready(Err(CryptoError::Unsealed {
            name: key.name().to_owned(),
        }))
    }
}

/// What a value is sealed for: the purpose its key is derived for, and the
/// data it is bound to, which must match again for it to open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sealing<'a> {
    /// The purpose its key is derived for.
    pub purpose: &'a str,
    /// The data it is bound to, as AES-GCM's associated data.
    pub bound: &'a [u8],
}

/// Checks a signature against the key or secret it is handed.
pub trait Verifier: Sync {
    /// Whether `signature` is `key`'s signature of `message` under
    /// `algorithm`. For HMAC the key is the shared secret's bytes; otherwise it
    /// is the public key.
    fn verify(
        &self,
        algorithm: Algorithm,
        key: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> impl Future<Output = bool> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_error_reads_as_a_sentence() {
        assert_eq!(
            CryptoError::BadKey {
                name: "KEY".to_owned()
            }
            .to_string(),
            "the secret KEY is not a key for this signature; give a private key as PEM, PKCS#8 or an RSA key's PKCS#1"
        );
    }
}
