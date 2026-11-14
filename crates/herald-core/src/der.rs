//! The DER and PEM forms public and private keys travel in.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

/// The DER tag of an `INTEGER`.
pub const INTEGER: u8 = 0x02;
/// The DER tag of a `BIT STRING`.
pub const BIT_STRING: u8 = 0x03;
/// The DER tag of a `SEQUENCE`.
pub const SEQUENCE: u8 = 0x30;
/// The DER `NULL` an algorithm's absent parameters are written as.
pub const NULL: [u8; 2] = [0x05, 0x00];
/// The DER object identifier of `rsaEncryption`.
pub const RSA_ENCRYPTION: [u8; 11] = [
    0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01,
];
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

/// The contents of the DER element of tag `tag` that `der` opens with, and
/// what follows it. A length is read in one byte, or in the one or two after
/// `0x81` or `0x82`, which an RSA key's lengths fit.
#[must_use]
pub fn element(der: &[u8], tag: u8) -> Option<(&[u8], &[u8])> {
    let (&found, rest) = der.split_first()?;
    let (length, rest) = match rest {
        [short @ 0..=0x7f, rest @ ..] => (usize::from(*short), rest),
        [0x81, long, rest @ ..] => (usize::from(*long), rest),
        [0x82, high, low, rest @ ..] => (usize::from(u16::from_be_bytes([*high, *low])), rest),
        _ => return None,
    };
    (found == tag && rest.len() >= length).then(|| rest.split_at(length))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
