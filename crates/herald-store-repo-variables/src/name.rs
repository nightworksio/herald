/// What every record's variable name starts with.
pub(crate) const PREFIX: &str = "HERALD_";
/// The variable whose existence is the lock.
pub(crate) const LOCK: &str = "HERALDLOCK";

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// The Base32 letter or digit of the low five bits of `value`.
fn symbol(value: u32) -> char {
    usize::try_from(value & 31)
        .ok()
        .and_then(|index| ALPHABET.get(index))
        .map_or('A', |letter| char::from(*letter))
}

/// The variable a record's path is kept in: its bytes in Base32, which
/// survives GitHub's case-insensitive names and uses only the letters and
/// digits a name may hold.
pub(crate) fn of(path: &str) -> String {
    let mut name = String::from(PREFIX);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for byte in path.bytes() {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            name.push(symbol(buffer >> bits));
        }
    }
    if bits > 0 {
        name.push(symbol(buffer << (5 - bits)));
    }
    name
}

/// The path a variable named `name` keeps, if it is one of herald's.
pub(crate) fn path(name: &str) -> Option<String> {
    let encoded = name.to_ascii_uppercase();
    let encoded = encoded.strip_prefix(PREFIX)?;
    let mut bytes = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for symbol in encoded.bytes() {
        let value = ALPHABET.iter().position(|letter| *letter == symbol)?;
        buffer = (buffer << 5) | u32::try_from(value).ok()?;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            bytes.push(u8::try_from((buffer >> bits) & 0xff).ok()?);
        }
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_becomes_a_name_of_letters_and_digits_and_back() {
        for kept in [
            "ledger/ab12.json",
            "pins/r.json",
            "subscriptions/slack/p/u~2f.json",
            "a",
        ] {
            let name = of(kept);
            assert!(
                name.bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'),
                "{name}"
            );
            assert_eq!(path(&name).as_deref(), Some(kept), "{name}");
            assert_eq!(path(&name.to_ascii_lowercase()).as_deref(), Some(kept));
        }
        assert_eq!(of("f"), "HERALD_MY");
        assert_eq!(of("foob"), "HERALD_MZXW6YQ");
        assert_eq!(of("foo"), "HERALD_MZXW6");
    }

    #[test]
    fn a_name_that_is_not_herald_s_keeps_no_path() {
        assert_eq!(path("DEPLOY_TARGET"), None);
        assert_eq!(path(LOCK), None);
        assert_eq!(path("HERALD_1"), None);
        assert_eq!(path("HERALD_7777"), None);
    }
}
