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
/// The label of a PKCS#1 RSA private key's PEM block, as GitHub hands out
/// an App's key.
pub const RSA_PRIVATE_KEY: &str = "RSA PRIVATE KEY";
/// The DER tag of an `OCTET STRING`.
pub const OCTET_STRING: u8 = 0x04;
/// The bytes a PKCS#8 key holds before the PKCS#1 key it wraps: its
/// `SEQUENCE`, version, algorithm and `OCTET STRING` headers, each length
/// in two bytes.
const PKCS8_HEAD: usize = 26;
/// The bytes of a PKCS#8 key's `SEQUENCE` after its length and before the
/// PKCS#1 key: its version, algorithm and `OCTET STRING` header.
const PKCS8_BETWEEN: u16 = 22;

/// The DER bytes one line of a PEM block holds: 64 characters of base64.
const PEM_LINE: usize = 48;

/// The PEM block of `der` labelled `label`, written onto `out` a line at a
/// time with no copy of its own, so a private key's block can be written
/// into a buffer its owner zeroes.
pub fn armour(der: &[u8], label: &str, out: &mut String) {
    out.push_str("-----BEGIN ");
    out.push_str(label);
    out.push_str("-----\n");
    for line in der.chunks(PEM_LINE) {
        STANDARD.encode_string(line, out);
        out.push('\n');
    }
    out.push_str("-----END ");
    out.push_str(label);
    out.push_str("-----\n");
}

/// The DER `text` holds as one PEM block labelled `label`, and nothing
/// around it, decoded four characters at a time into a buffer allocated
/// once, so no other copy of a private key is left on the heap.
#[must_use]
pub fn pem(text: &str, label: &str) -> Option<Vec<u8>> {
    decoded(text, label, 0)
}

/// The PKCS#8 DER of the private key `text` holds as one PEM block: a
/// PKCS#8 key as it is, or a PKCS#1 RSA key wrapped in PKCS#8 in the buffer
/// it is decoded into, so no other copy of it is left on the heap.
#[must_use]
pub fn private_key(text: &str) -> Option<Vec<u8>> {
    if let Some(der) = pem(text, PRIVATE_KEY) {
        return Some(der);
    }
    let mut der = decoded(text, RSA_PRIVATE_KEY, PKCS8_HEAD)?;
    let key = u16::try_from(der.len() - PKCS8_HEAD)
        .ok()
        .filter(|key| *key > 0xff)?;
    let [whole_high, whole_low] = key.checked_add(PKCS8_BETWEEN)?.to_be_bytes();
    let [key_high, key_low] = key.to_be_bytes();
    let mut head = vec![SEQUENCE, 0x82, whole_high, whole_low, INTEGER, 0x01, 0x00];
    head.extend_from_slice(&[SEQUENCE, 0x0d]);
    head.extend_from_slice(&RSA_ENCRYPTION);
    head.extend_from_slice(&NULL);
    head.extend_from_slice(&[OCTET_STRING, 0x82, key_high, key_low]);
    der.get_mut(..PKCS8_HEAD)?.copy_from_slice(&head);
    Some(der)
}

/// The DER `text` holds as one PEM block labelled `label`, decoded after
/// `room` zero bytes.
fn decoded(text: &str, label: &str, room: usize) -> Option<Vec<u8>> {
    let inner = text
        .trim()
        .strip_prefix(&format!("-----BEGIN {label}-----"))?
        .strip_suffix(&format!("-----END {label}-----"))?;
    if inner.contains("-----") {
        return None;
    }
    let mut der = Vec::with_capacity(room + inner.len() / 4 * 3);
    der.resize(room, 0);
    let mut quad = [0; 4];
    let mut held = 0;
    let mut padded = false;
    for byte in inner.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
        if padded {
            return None;
        }
        *quad.get_mut(held)? = byte;
        held += 1;
        if held == quad.len() {
            let mut bytes = [0; 3];
            let decoded = STANDARD.decode_slice(quad, &mut bytes).ok()?;
            der.extend_from_slice(bytes.get(..decoded)?);
            padded = decoded < bytes.len();
            held = 0;
        }
    }
    (held == 0).then_some(der)
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
    fn a_block_is_armoured_a_line_of_sixty_four_characters_at_a_time_and_read_back() {
        let der: Vec<u8> = (0..=99).collect();
        let mut block = String::new();
        armour(&der, PRIVATE_KEY, &mut block);
        let lines: Vec<&str> = block.lines().collect();
        assert_eq!(lines.first(), Some(&"-----BEGIN PRIVATE KEY-----"));
        assert_eq!(lines.last(), Some(&"-----END PRIVATE KEY-----"));
        assert_eq!(
            lines[1..lines.len() - 1]
                .iter()
                .map(|line| line.len())
                .collect::<Vec<_>>(),
            [64, 64, 8]
        );
        assert_eq!(pem(&block, PRIVATE_KEY), Some(der));
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
    fn a_private_key_is_read_as_pkcs8_or_wrapped_into_it_from_pkcs1() {
        let pkcs8 = armoured(PRIVATE_KEY, "AAEC", PRIVATE_KEY);
        assert_eq!(private_key(&pkcs8), Some(vec![0, 1, 2]));
        let key = vec![7; 0x100];
        let pkcs1 = armoured(RSA_PRIVATE_KEY, &STANDARD.encode(&key), RSA_PRIVATE_KEY);
        let wrapped = private_key(&pkcs1).unwrap();
        let (inner, rest) = element(&wrapped, SEQUENCE).unwrap();
        assert_eq!(rest, <&[u8]>::default());
        let (version, inner) = element(inner, INTEGER).unwrap();
        assert_eq!(version, [0]);
        let (algorithm, inner) = element(inner, SEQUENCE).unwrap();
        assert_eq!(algorithm, [&RSA_ENCRYPTION[..], &NULL[..]].concat());
        assert_eq!(element(inner, OCTET_STRING), Some((&key[..], &[][..])));
        let short = armoured(
            RSA_PRIVATE_KEY,
            &STANDARD.encode([7; 0xff]),
            RSA_PRIVATE_KEY,
        );
        assert_eq!(private_key(&short), None);
        let long = armoured(
            RSA_PRIVATE_KEY,
            &STANDARD.encode(vec![7; 0xffea]),
            RSA_PRIVATE_KEY,
        );
        assert_eq!(private_key(&long), None);
        assert_eq!(private_key(&armoured(PUBLIC_KEY, "AAEC", PUBLIC_KEY)), None);
    }

    #[test]
    fn a_pem_body_is_read_across_any_whitespace_and_ends_at_its_padding() {
        let read = |body: &str| pem(&armoured(PRIVATE_KEY, body, PRIVATE_KEY), PRIVATE_KEY);
        assert_eq!(read("AA\tE C\r\nAAEC"), Some(vec![0, 1, 2, 0, 1, 2]));
        assert_eq!(read(""), Some(vec![]));
        assert_eq!(read("Aw==AAEC"), None);
        assert_eq!(read("AAECAw"), None);
        assert_eq!(read("AAE"), None);
        let held = read("AAECAAECAAEC").unwrap();
        assert_eq!((held.len(), held.capacity()), (9, 9));
    }
}
