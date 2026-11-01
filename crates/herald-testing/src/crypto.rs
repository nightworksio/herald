use herald_ports::{Algorithm, CryptoError, Secret, Signer, Verifier};

/// A signer whose signature spells out what was signed, with which key: the
/// algorithm, the key's value and the message, joined by `|`. A key named
/// `BAD` is refused.
#[derive(Debug, Default)]
pub struct FakeSigner;

/// A verifier that accepts exactly the signatures [`FakeSigner`] makes for
/// the key it is handed.
#[derive(Debug, Default)]
pub struct FakeVerifier;

fn spelled(algorithm: Algorithm, key: &[u8], message: &[u8]) -> Vec<u8> {
    [format!("{algorithm:?}").as_bytes(), key, message].join(&b'|')
}

impl Signer for FakeSigner {
    async fn sign(
        &self,
        algorithm: Algorithm,
        key: &Secret,
        message: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        if key.name() == "BAD" {
            return Err(CryptoError::BadKey {
                name: key.name().to_owned(),
            });
        }
        Ok(spelled(algorithm, key.expose().as_bytes(), message))
    }
}

impl Verifier for FakeVerifier {
    async fn verify(
        &self,
        algorithm: Algorithm,
        key: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> bool {
        spelled(algorithm, key, message) == signature
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_verifier_accepts_the_signers_signature_and_nothing_else() {
        let key = Secret::new("KEY", "k");
        let signature =
            pollster::block_on(FakeSigner.sign(Algorithm::Ed25519, &key, b"m")).unwrap();
        assert_eq!(signature, b"Ed25519|k|m");
        assert!(pollster::block_on(FakeVerifier.verify(
            Algorithm::Ed25519,
            b"k",
            b"m",
            &signature
        )));
        assert!(!pollster::block_on(FakeVerifier.verify(
            Algorithm::HmacSha256,
            b"k",
            b"m",
            &signature
        )));
        assert!(!pollster::block_on(FakeVerifier.verify(
            Algorithm::Ed25519,
            b"x",
            b"m",
            &signature
        )));
    }

    #[test]
    fn a_bad_key_is_refused() {
        let key = Secret::new("BAD", "k");
        assert_eq!(
            pollster::block_on(FakeSigner.sign(Algorithm::RsaSha256, &key, b"m")),
            Err(CryptoError::BadKey {
                name: "BAD".to_owned()
            })
        );
    }
}
