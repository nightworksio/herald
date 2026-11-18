use herald_ports::{Algorithm, CryptoError, Sealing, Secret, Signer, Verifier};

/// A signer whose signature spells out what was signed, with which key: the
/// algorithm, the key's value and the message, joined by `|`. A key named
/// `BAD` is refused.
#[derive(Debug, Clone, Copy, Default)]
pub struct FakeSigner;

/// A verifier that accepts exactly the signatures [`FakeSigner`] makes for
/// the key it is handed, and the Ed25519-sized ones [`fake_ed25519`] makes.
#[derive(Debug, Default)]
pub struct FakeVerifier;

/// The DER of an Ed25519 `SubjectPublicKeyInfo` before its 32-byte key.
const ED25519_SUBJECT: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];
/// The bytes of an Ed25519 public key.
const ED25519_LENGTH: usize = 32;

/// The bytes of an Ed25519 signature.
const SIGNATURE_LENGTH: usize = 64;

/// A stand-in for the Ed25519 signature of `message` by the key whose
/// public half is `key`: 64 bytes folded from both, so what checks a
/// signature's length takes it, and [`FakeVerifier`] accepts it for `key`
/// alone.
#[must_use]
pub fn fake_ed25519(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut folded = vec![0_u8; SIGNATURE_LENGTH];
    let signed = [key, message].concat();
    for (round, chunk) in (0_u8..=u8::MAX)
        .cycle()
        .zip(signed.chunks(SIGNATURE_LENGTH))
    {
        for (slot, byte) in folded.iter_mut().zip(chunk) {
            *slot = slot.rotate_left(3) ^ byte ^ round;
        }
    }
    folded
}

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

    /// `sealed|`, the key's value, the purpose, the bound data and `plain`,
    /// joined by `|`.
    async fn seal(
        &self,
        key: &Secret,
        sealing: Sealing<'_>,
        plain: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        Ok([
            b"sealed",
            key.expose().as_bytes(),
            sealing.purpose.as_bytes(),
            sealing.bound,
            plain,
        ]
        .join(&b'|'))
    }

    /// What [`FakeSigner::seal`] sealed under `key` as `sealing` says.
    async fn open(
        &self,
        key: &Secret,
        sealing: Sealing<'_>,
        sealed: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let head = [
            b"sealed",
            key.expose().as_bytes(),
            sealing.purpose.as_bytes(),
            sealing.bound,
            b"",
        ]
        .join(&b'|');
        sealed
            .strip_prefix(head.as_slice())
            .map(<[u8]>::to_vec)
            .ok_or_else(|| CryptoError::Unsealed {
                name: key.name().to_owned(),
            })
    }

    /// `public|`, the algorithm and the key's value, joined by `|`; for an
    /// Ed25519 key whose value is 32 bytes, those bytes as the key of a
    /// `SubjectPublicKeyInfo`, as a real Ed25519 key's public half is.
    async fn public(&self, algorithm: Algorithm, key: &Secret) -> Result<Vec<u8>, CryptoError> {
        if key.name() == "BAD" {
            return Err(CryptoError::BadKey {
                name: key.name().to_owned(),
            });
        }
        let value = key.expose().as_bytes();
        if algorithm == Algorithm::Ed25519 && value.len() == ED25519_LENGTH {
            return Ok([&ED25519_SUBJECT[..], value].concat());
        }
        Ok(spelled(algorithm, b"public", value))
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
            || (algorithm == Algorithm::Ed25519 && fake_ed25519(key, message) == signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_verifier_accepts_a_fake_ed25519_signature_for_its_key_alone() {
        let signature = fake_ed25519(b"key", b"message");
        assert_eq!(signature.len(), 64);
        assert!(pollster::block_on(FakeVerifier.verify(
            Algorithm::Ed25519,
            b"key",
            b"message",
            &signature
        )));
        for (algorithm, key, message) in [
            (Algorithm::HmacSha256, &b"key"[..], &b"message"[..]),
            (Algorithm::Ed25519, b"kez", b"message"),
            (Algorithm::Ed25519, b"key", b"messagf"),
        ] {
            assert!(!pollster::block_on(
                FakeVerifier.verify(algorithm, key, message, &signature)
            ));
        }
        assert_ne!(fake_ed25519(b"xy", b""), fake_ed25519(b"yx", b""));
        assert_ne!(fake_ed25519(&[0; 64], b""), fake_ed25519(&[0; 65], b""));
        let mut wrapped = vec![0; 63];
        wrapped.push(9);
        assert_eq!(fake_ed25519(&[1], &wrapped)[0], 0);
    }

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
    fn the_public_half_spells_the_algorithm_and_the_key_and_a_bad_key_has_none() {
        let key = Secret::new("KEY", "k");
        assert_eq!(
            pollster::block_on(FakeSigner.public(Algorithm::Ed25519, &key)).unwrap(),
            b"Ed25519|public|k"
        );
        let wide = Secret::new("WIDE", "k".repeat(ED25519_LENGTH));
        assert_eq!(
            pollster::block_on(FakeSigner.public(Algorithm::Ed25519, &wide)).unwrap(),
            [&ED25519_SUBJECT[..], "k".repeat(ED25519_LENGTH).as_bytes()].concat()
        );
        assert_eq!(
            pollster::block_on(FakeSigner.public(Algorithm::RsaSha256, &wide)).unwrap(),
            [
                b"RsaSha256|public|".as_slice(),
                "k".repeat(ED25519_LENGTH).as_bytes()
            ]
            .concat()
        );
        let bad = Secret::new("BAD", "k");
        assert_eq!(
            pollster::block_on(FakeSigner.public(Algorithm::Ed25519, &bad)),
            Err(CryptoError::BadKey {
                name: "BAD".to_owned()
            })
        );
    }

    #[test]
    fn a_sealed_value_spells_its_key_and_purpose_and_opens_only_under_them() {
        let key = Secret::new("KEY", "k");
        let with = |purpose, bound| Sealing { purpose, bound };
        let sealed = pollster::block_on(FakeSigner.seal(&key, with("p", b"b"), b"plain")).unwrap();
        assert_eq!(sealed, b"sealed|k|p|b|plain");
        assert_eq!(
            pollster::block_on(FakeSigner.open(&key, with("p", b"b"), &sealed)),
            Ok(b"plain".to_vec())
        );
        assert_eq!(
            pollster::block_on(FakeSigner.open(&key, with("p", b"c"), &sealed)),
            Err(CryptoError::Unsealed {
                name: "KEY".to_owned()
            })
        );
        assert_eq!(
            pollster::block_on(FakeSigner.open(&key, with("other", b"b"), &sealed)),
            Err(CryptoError::Unsealed {
                name: "KEY".to_owned()
            })
        );
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
