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
}

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadKey { name } => write!(
                f,
                "the secret {name} is not a key for this signature; give a PKCS#8 private key as PEM"
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
            "the secret KEY is not a key for this signature; give a PKCS#8 private key as PEM"
        );
    }
}
