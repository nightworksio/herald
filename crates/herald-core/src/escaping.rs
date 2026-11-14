//! Text written so that only a set of bytes stays as it is, every other
//! byte as a mark and its two hex digits, and read back.

use std::fmt::Write as _;

/// Which bytes stay as they are, and how every other byte is written.
#[derive(Debug, Clone, Copy)]
pub struct Escaping {
    /// Whether a byte stays as it is.
    pub keeps: fn(u8) -> bool,
    /// What is written before the two hex digits of a byte that does not.
    pub mark: char,
    /// Whether the hex digits are uppercase.
    pub upper: bool,
}

impl Escaping {
    /// A URL's unreserved bytes kept, and every other percent-encoded: fit
    /// for one segment of a path or one value of a query.
    pub const UNRESERVED: Self = Self {
        keeps: |byte| byte.is_ascii_alphanumeric() || b"-._~".contains(&byte),
        mark: '%',
        upper: true,
    };

    /// Every visible ASCII byte kept but those that could end a Markdown
    /// link or open markup, and every other percent-encoded.
    pub(crate) const LINK: Self = Self {
        keeps: |byte| byte.is_ascii_graphic() && !b"\"'<>\\^`{|}()[]".contains(&byte),
        mark: '%',
        upper: true,
    };

    /// `text`, with every byte this escaping does not keep written as its
    /// mark and two hex digits.
    #[must_use]
    pub fn escape(self, text: &str) -> String {
        text.bytes().fold(String::new(), |mut escaped, byte| {
            if (self.keeps)(byte) {
                escaped.push(char::from(byte));
            } else if self.upper {
                let _ = write!(escaped, "{}{byte:02X}", self.mark);
            } else {
                let _ = write!(escaped, "{}{byte:02x}", self.mark);
            }
            escaped
        })
    }

    /// `text`, with each mark and the two hex digits after it read as the
    /// byte they write; nothing where a mark is not followed by two hex
    /// digits, or the bytes are not UTF-8.
    #[must_use]
    pub fn unescape(self, text: &str) -> Option<String> {
        let mut bytes = Vec::with_capacity(text.len());
        let mut rest = text;
        while let Some((before, after)) = rest.split_once(self.mark) {
            bytes.extend_from_slice(before.as_bytes());
            let digits = after.get(..2)?;
            bytes.extend(crate::unhex(digits)?);
            rest = &after[2..];
        }
        bytes.extend_from_slice(rest.as_bytes());
        String::from_utf8(bytes).ok()
    }
}

/// `text` with every byte but a URL's unreserved ones percent-encoded, fit
/// for one segment of a path or one value of a query.
#[must_use]
pub fn unreserved(text: &str) -> String {
    Escaping::UNRESERVED.escape(text)
}

/// `url` with every byte that could end a Markdown link or open markup in
/// it percent-encoded.
#[must_use]
pub fn safe_url(url: &str) -> String {
    Escaping::LINK.escape(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const NAME: Escaping = Escaping {
        keeps: |byte| byte.is_ascii_alphanumeric(),
        mark: '~',
        upper: false,
    };

    #[test]
    fn every_byte_but_a_url_s_unreserved_ones_is_percent_encoded() {
        assert_eq!(unreserved("aZ09-._~"), "aZ09-._~");
        assert_eq!(unreserved("a b/c?d&é"), "a%20b%2Fc%3Fd%26%C3%A9");
        assert_eq!(unreserved("@scope/name"), "%40scope%2Fname");
        assert_eq!(unreserved(""), "");
    }

    #[test]
    fn a_link_keeps_what_a_url_writes_and_escapes_what_ends_markup() {
        assert_eq!(
            safe_url("https://example.org/a?b=c#d"),
            "https://example.org/a?b=c#d"
        );
        assert_eq!(safe_url("https://x/(a) [b]"), "https://x/%28a%29%20%5Bb%5D");
        assert_eq!(
            safe_url("https://x/é\"'<>\\^`{|}"),
            "https://x/%C3%A9%22%27%3C%3E%5C%5E%60%7B%7C%7D"
        );
    }

    #[test]
    fn a_mark_and_the_case_of_its_digits_are_the_escaping_s_own() {
        assert_eq!(NAME.escape("a/B~é"), "a~2fB~7e~c3~a9");
        assert_eq!(NAME.unescape("a~2fB~7e~c3~a9").as_deref(), Some("a/B~é"));
    }

    #[test]
    fn a_mark_and_two_hex_digits_read_as_their_byte() {
        let percent = Escaping::UNRESERVED;
        assert_eq!(
            percent.unescape("acct%3Aana%40social.example").as_deref(),
            Some("acct:ana@social.example")
        );
        assert_eq!(percent.unescape("%2f%2F").as_deref(), Some("//"));
        assert_eq!(percent.unescape("plain").as_deref(), Some("plain"));
        for broken in ["%", "%4", "%zz", "%ff", "%+5", "%-1", "a%é1"] {
            assert_eq!(percent.unescape(broken), None, "{broken}");
        }
    }

    proptest! {
        #[test]
        fn what_is_escaped_holds_only_kept_bytes_and_reads_back(text in "\\PC{0,24}") {
            for escaping in [Escaping::UNRESERVED, NAME] {
                let escaped = escaping.escape(&text);
                let mark = u8::try_from(escaping.mark).unwrap_or_default();
                prop_assert!(escaped.bytes().all(|byte| (escaping.keeps)(byte) || byte == mark || byte.is_ascii_hexdigit()));
                prop_assert_eq!(escaping.unescape(&escaped), Some(text.clone()));
            }
        }

        #[test]
        fn a_link_holds_only_visible_ascii(text in "\\PC{0,24}") {
            prop_assert!(safe_url(&text).bytes().all(|byte| byte.is_ascii_graphic()));
        }
    }
}
