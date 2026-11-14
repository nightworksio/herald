//! The encodings herald's keys travel in, whichever library signs with
//! them: a PKCS#8 private key in PEM, the DER `SubjectPublicKeyInfo` an
//! ActivityPub actor publishes, and the PKCS#1 public key an RSA signature
//! is checked with.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use herald_core::der::{BIT_STRING, INTEGER, NULL, RSA_ENCRYPTION, SEQUENCE};

/// The DER `AlgorithmIdentifier` of Ed25519.
const ED25519: [u8; 7] = [0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70];

/// A kind of public key a `SubjectPublicKeyInfo` holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Public {
    /// An Ed25519 key, as its 32 bytes.
    Ed25519,
    /// An RSA key, as its PKCS#1 `RSAPublicKey`.
    Rsa,
}

pub use herald_core::der::{PRIVATE_KEY, PUBLIC_KEY, pem};

/// The bytes of a JSON Web Key's base64url member.
#[must_use]
pub fn jwk_bytes(member: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(member).ok()
}

/// The `SubjectPublicKeyInfo` of the public key `key` of the kind `public`.
#[must_use]
pub fn subject(public: Public, key: &[u8]) -> Vec<u8> {
    let rsa;
    let algorithm: &[u8] = match public {
        Public::Ed25519 => &ED25519,
        Public::Rsa => {
            rsa = der(SEQUENCE, &[&RSA_ENCRYPTION[..], &NULL].concat());
            &rsa
        }
    };
    let bits = der(BIT_STRING, &[&[0], key].concat());
    der(SEQUENCE, &[algorithm, &bits].concat())
}

/// The PKCS#1 `RSAPublicKey` of the modulus `modulus` and the public
/// exponent `exponent`, each big-endian.
#[must_use]
pub fn rsa_public(modulus: &[u8], exponent: &[u8]) -> Vec<u8> {
    der(SEQUENCE, &[integer(modulus), integer(exponent)].concat())
}

/// The unsigned big-endian `value` as one DER `INTEGER`.
fn integer(value: &[u8]) -> Vec<u8> {
    let trimmed: Vec<u8> = value
        .iter()
        .copied()
        .skip_while(|byte| *byte == 0)
        .collect();
    let positive = match trimmed.first() {
        Some(first) if *first < 0x80 => trimmed,
        _ => [&[0], trimmed.as_slice()].concat(),
    };
    der(INTEGER, &positive)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_der_length_is_short_below_128_bytes_and_long_from_there() {
        let value = |length: usize| der(SEQUENCE, &vec![7; length]);
        assert_eq!(value(0), [SEQUENCE, 0]);
        assert_eq!(value(127)[..2], [SEQUENCE, 127]);
        assert_eq!(value(128)[..3], [SEQUENCE, 0x81, 0x80]);
        assert_eq!(value(255)[..3], [SEQUENCE, 0x81, 0xff]);
        assert_eq!(value(256)[..4], [SEQUENCE, 0x82, 0x01, 0x00]);
        assert_eq!(value(65_536)[..5], [SEQUENCE, 0x83, 0x01, 0x00, 0x00]);
        assert_eq!(value(300).len(), 304);
    }

    #[test]
    fn a_jwk_member_is_base64url_without_padding() {
        assert_eq!(jwk_bytes("_-8"), Some(vec![0xff, 0xef]));
        assert_eq!(jwk_bytes("_-8="), None);
    }

    #[test]
    fn an_ed25519_key_is_published_under_its_algorithm() {
        let spki = subject(Public::Ed25519, &[7; 32]);
        assert_eq!(
            spki.get(..12),
            Some(
                &[
                    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00
                ][..]
            )
        );
        assert_eq!(spki.get(12..), Some(&[7; 32][..]));
    }

    #[test]
    fn a_long_key_takes_the_long_form_of_its_length() {
        let spki = subject(Public::Rsa, &[1; 270]);
        assert_eq!(spki.get(..4), Some(&[0x30, 0x82, 0x01, 0x22][..]));
        assert_eq!(spki.get(6..17), Some(&RSA_ENCRYPTION[..]));
        assert_eq!(spki.get(19..24), Some(&[0x03, 0x82, 0x01, 0x0f, 0x00][..]));
    }

    #[test]
    fn an_rsa_key_s_integers_stay_positive_and_minimal() {
        assert_eq!(
            rsa_public(&[0x00, 0x80, 0x01], &[0x01, 0x00, 0x01]),
            [
                0x30, 0x0a, 0x02, 0x03, 0x00, 0x80, 0x01, 0x02, 0x03, 0x01, 0x00, 0x01
            ]
        );
        assert_eq!(
            rsa_public(&[0x7f], &[]),
            [0x30, 0x06, 0x02, 0x01, 0x7f, 0x02, 0x01, 0x00]
        );
    }
}
