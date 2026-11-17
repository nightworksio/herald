//! Bytes written as multibase base58btc (`z` then Bitcoin's base58), and
//! an Ed25519 public key written as a Multikey, as Data Integrity proofs
//! and their verification methods carry them.

/// Bitcoin's base58 alphabet.
const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
/// The multibase prefix of base58btc.
const BASE58BTC: char = 'z';
/// The multicodec prefix of an Ed25519 public key, `ed25519-pub` (0xed) as
/// an unsigned varint.
const ED25519_PUB: [u8; 2] = [0xed, 0x01];
/// The length of an Ed25519 public key.
pub const ED25519_LENGTH: usize = 32;
/// The radix of base58.
const RADIX: u32 = 58;
/// The bytes an Ed25519 Multikey decodes to: its multicodec prefix, then
/// the key.
pub const MULTIKEY_LENGTH: usize = ED25519_PUB.len() + ED25519_LENGTH;
/// The base58 digits a byte's carry spans once the digits before it took
/// their share: it stays under 256, and 58² is past it.
const CARRY_DIGITS: usize = 2;
/// Base58 characters per thousand bytes, rounded up: log(256) / log(58) is
/// 1.3657.
const CHARACTERS_PER_THOUSAND: usize = 1366;
/// Bytes in the thousand [`CHARACTERS_PER_THOUSAND`] counts by.
const THOUSAND: usize = 1000;

/// `bytes` as multibase base58btc.
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    let zeros = bytes.iter().take_while(|byte| **byte == 0).count();
    let mut digits: Vec<u8> = Vec::new();
    for byte in bytes.iter().skip(zeros) {
        let mut carry = u32::from(*byte);
        for digit in &mut digits {
            carry += u32::from(*digit) << 8;
            *digit = u8::try_from(carry % RADIX).unwrap_or_default();
            carry /= RADIX;
        }
        for _ in 0..CARRY_DIGITS {
            if carry > 0 {
                digits.push(u8::try_from(carry % RADIX).unwrap_or_default());
                carry /= RADIX;
            }
        }
    }
    let mut out = String::with_capacity(1 + zeros + digits.len());
    out.push(BASE58BTC);
    out.extend(std::iter::repeat_n('1', zeros));
    out.extend(
        digits
            .iter()
            .rev()
            .filter_map(|digit| ALPHABET.get(usize::from(*digit)).map(|&c| char::from(c))),
    );
    out
}

/// The most base58 characters `most` bytes are written with, leading zero
/// bytes included, as each takes one character.
#[must_use]
pub fn longest(most: usize) -> usize {
    most.saturating_mul(CHARACTERS_PER_THOUSAND) / THOUSAND + 1
}

/// The bytes the multibase base58btc `text` writes, if it is that and they
/// number at most `most`. Text longer than `most` bytes are written with is
/// refused before it is decoded, as decoding takes time growing with the
/// square of its length.
#[must_use]
pub fn decode(text: &str, most: usize) -> Option<Vec<u8>> {
    let written = text.strip_prefix(BASE58BTC)?;
    if written.len() > longest(most) {
        return None;
    }
    let zeros = written.bytes().take_while(|byte| *byte == b'1').count();
    let mut bytes: Vec<u8> = Vec::new();
    for character in written.bytes().skip(zeros) {
        let mut carry = u32::try_from(ALPHABET.iter().position(|c| *c == character)?).ok()?;
        for byte in &mut bytes {
            carry += u32::from(*byte) * RADIX;
            *byte = u8::try_from(carry & 0xff).unwrap_or_default();
            carry >>= 8;
        }
        if carry > 0 {
            bytes.push(u8::try_from(carry).unwrap_or_default());
        }
    }
    let mut out = vec![0; zeros];
    out.extend(bytes.iter().rev());
    (out.len() <= most).then_some(out)
}

/// The Ed25519 public key `key` as a Multikey's `publicKeyMultibase`.
#[must_use]
pub fn ed25519_public(key: &[u8; ED25519_LENGTH]) -> String {
    encode(&[&ED25519_PUB[..], key].concat())
}

/// The Ed25519 public key a Multikey's `publicKeyMultibase` `text` writes,
/// if it writes one.
#[must_use]
pub fn ed25519_key(text: &str) -> Option<[u8; ED25519_LENGTH]> {
    decode(text, MULTIKEY_LENGTH)?
        .strip_prefix(&ED25519_PUB[..])?
        .try_into()
        .ok()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn bytes_are_written_as_bitcoin_s_base58_after_a_z() {
        for (bytes, text) in [
            (&b""[..], "z"),
            (&[0][..], "z1"),
            (&[0, 0, 1][..], "z112"),
            (b"Hello World!", "z2NEpo7TZRRrLZSi2U"),
            (
                b"The quick brown fox jumps over the lazy dog.",
                "zUSm3fpXnKG5EUBx2ndxBDMPVciP5hGey2Jh4NDv6gmeo1LkMeiKrLJUUBk6Z",
            ),
        ] {
            assert_eq!(encode(bytes), text);
            assert_eq!(decode(text, bytes.len()).as_deref(), Some(bytes));
        }
    }

    #[test]
    fn text_without_the_prefix_or_outside_the_alphabet_writes_nothing() {
        for text in ["", "2NEpo7TZRRrLZSi2U", "f00", "z0", "zIl", "zO"] {
            assert_eq!(decode(text, 8), None, "{text}");
        }
    }

    #[test]
    fn text_longer_than_its_most_bytes_need_is_refused_before_it_is_decoded() {
        let widest = encode(&[0xff; MULTIKEY_LENGTH]);
        assert_eq!(widest.len() - 1, longest(MULTIKEY_LENGTH));
        assert_eq!(
            decode(&widest, MULTIKEY_LENGTH),
            Some(vec![0xff; MULTIKEY_LENGTH])
        );
        assert_eq!(decode(&widest, MULTIKEY_LENGTH - 1), None);
        let zeros = format!("z{}", "1".repeat(longest(MULTIKEY_LENGTH)));
        assert_eq!(decode(&zeros, MULTIKEY_LENGTH), None);
        assert_eq!(
            decode(&zeros, longest(MULTIKEY_LENGTH)),
            Some(vec![0; longest(MULTIKEY_LENGTH)])
        );
        let flood = format!("z{}", "2".repeat(1_000_000));
        assert_eq!(decode(&flood, MULTIKEY_LENGTH), None);
        assert_eq!(longest(0), 1);
        assert_eq!(longest(1000), 1367);
    }

    #[test]
    fn a_multikey_is_its_two_byte_prefix_and_its_key() {
        assert_eq!(MULTIKEY_LENGTH, 34);
    }

    #[test]
    fn an_ed25519_key_is_a_multikey_starting_z6mk() {
        let key = [0x11; ED25519_LENGTH];
        let written = ed25519_public(&key);
        assert!(written.starts_with("z6Mk"), "{written}");
        assert_eq!(ed25519_key(&written), Some(key));
    }

    #[test]
    fn a_multikey_of_another_codec_or_length_is_no_ed25519_key() {
        let key = [0x11; ED25519_LENGTH];
        assert_eq!(
            ed25519_key(&encode(&[&[0xe7, 0x01][..], &key].concat())),
            None
        );
        assert_eq!(
            ed25519_key(&encode(&[&ED25519_PUB[..], &key[1..]].concat())),
            None
        );
        assert_eq!(ed25519_key("nope"), None);
    }

    proptest! {
        #[test]
        fn decoding_reads_back_what_encoding_wrote(bytes in prop::collection::vec(any::<u8>(), 0..64)) {
            prop_assert_eq!(decode(&encode(&bytes), bytes.len()), Some(bytes));
        }
    }
}
