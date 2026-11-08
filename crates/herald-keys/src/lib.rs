//! The encodings herald's keys travel in, whichever library signs with
//! them: a PKCS#8 private key in PEM, the DER `SubjectPublicKeyInfo` an
//! ActivityPub actor publishes, and the PKCS#1 public key an RSA signature
//! is checked with.

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};

/// The DER `AlgorithmIdentifier` of `rsaEncryption`, with its null
/// parameters.
const RSA_ENCRYPTION: [u8; 15] = [
    0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00,
];
/// The DER `AlgorithmIdentifier` of Ed25519.
const ED25519: [u8; 7] = [0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70];
const INTEGER: u8 = 0x02;
const BIT_STRING: u8 = 0x03;
const SEQUENCE: u8 = 0x30;

/// A kind of public key a `SubjectPublicKeyInfo` holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Public {
    /// An Ed25519 key, as its 32 bytes.
    Ed25519,
    /// An RSA key, as its PKCS#1 `RSAPublicKey`.
    Rsa,
}

/// The label of a PKCS#8 private key's PEM block.
pub const PRIVATE_KEY: &str = "PRIVATE KEY";
/// The label of a `SubjectPublicKeyInfo`'s PEM block.
pub const PUBLIC_KEY: &str = "PUBLIC KEY";

/// The DER `text` holds as one PEM block labelled `label`, and nothing
/// around it.
#[must_use]
pub fn pem(text: &str, label: &str) -> Option<Vec<u8>> {
    let inner = text
        .trim()
        .strip_prefix(&format!("-----BEGIN {label}-----"))?
        .strip_suffix(&format!("-----END {label}-----"))?;
    if inner.contains("-----") {
        return None;
    }
    let body: String = inner.split_whitespace().collect();
    STANDARD.decode(body).ok()
}

/// The bytes of a JSON Web Key's base64url member.
#[must_use]
pub fn jwk_bytes(member: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(member).ok()
}

/// The `SubjectPublicKeyInfo` of the public key `key` of the kind `public`.
#[must_use]
pub fn subject(public: Public, key: &[u8]) -> Vec<u8> {
    let algorithm: &[u8] = match public {
        Public::Ed25519 => &ED25519,
        Public::Rsa => &RSA_ENCRYPTION,
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

    /// `body` between a PEM `BEGIN` line of `begin` and an `END` line of `end`.
    fn armoured(begin: &str, body: &str, end: &str) -> String {
        format!("-----BEGIN {begin}-----\n{body}\n-----END {end}-----\n")
    }

    #[test]
    fn one_pem_block_of_the_label_asked_for_is_read_and_nothing_else() {
        let block = armoured(PRIVATE_KEY, " AAEC\nAw==", PRIVATE_KEY);
        assert_eq!(pem(&block, PRIVATE_KEY), Some(vec![0, 1, 2, 3]));
        assert_eq!(pem(&block, PUBLIC_KEY), None);
        assert_eq!(pem("AAEC", PRIVATE_KEY), None);
        let two = format!("{block}{block}");
        assert_eq!(pem(&two, PRIVATE_KEY), None);
        let mixed = armoured(PRIVATE_KEY, "AAEC", PUBLIC_KEY);
        assert_eq!(pem(&mixed, PRIVATE_KEY), None);
        let broken = armoured(PRIVATE_KEY, "not base64!", PRIVATE_KEY);
        assert_eq!(pem(&broken, PRIVATE_KEY), None);
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
        assert_eq!(spki.get(4..19), Some(&RSA_ENCRYPTION[..]));
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
