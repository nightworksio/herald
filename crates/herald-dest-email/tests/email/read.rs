//! A strict reader of a message as RFC 5322, RFC 2045, RFC 2046 and RFC 2047
//! read it, written apart from the writer it judges: it reads only what those
//! documents say a reader reads, and panics on anything they do not allow.
//! The fuzz target `email` reads through this file too.

/// A message as a reader sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    /// Each header field's name and its value, unfolded, with the
    /// whitespace before it removed.
    pub fields: Vec<(String, String)>,
    /// Each body part's media type and its decoded body.
    pub parts: Vec<(String, String)>,
}

impl Read {
    /// The header fields' names, in order.
    pub fn names(&self) -> Vec<&str> {
        self.fields.iter().map(|(name, _)| name.as_str()).collect()
    }

    /// The value of the one field named `name`.
    pub fn value(&self, name: &str) -> &str {
        let values: Vec<&str> = self
            .fields
            .iter()
            .filter(|(field, _)| field.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(values.len(), 1, "{name} stands once");
        values[0]
    }
}

/// A mailbox as a reader sees it: its display name and address.
pub type Seen = (Option<String>, String);

/// `message` read, every rule the RFCs give a writer checked.
pub fn read(message: &str) -> Read {
    for (at, byte) in message.bytes().enumerate() {
        assert!(
            (b' '..=b'~').contains(&byte) || matches!(byte, b'\r' | b'\n' | b'\t'),
            "byte {byte:#04x} at {at} is not 7-bit text"
        );
        if byte == b'\r' {
            assert_eq!(
                message.as_bytes().get(at + 1),
                Some(&b'\n'),
                "a CR ends a line"
            );
        }
        if byte == b'\n' {
            assert_eq!(
                message.as_bytes().get(at.wrapping_sub(1)),
                Some(&b'\r'),
                "an LF ends a line"
            );
        }
    }
    assert!(message.ends_with("\r\n"), "the message ends its last line");
    let (head, body) = message
        .split_once("\r\n\r\n")
        .expect("a blank line ends the header");
    let lines: Vec<&str> = head.split("\r\n").collect();
    for line in message.split("\r\n") {
        assert!(line.len() <= 998, "a line holds at most 998 characters");
    }
    for line in &lines {
        if line
            .split([' ', '\t'])
            .any(|token| token.starts_with("=?") && token.ends_with("?="))
        {
            assert!(
                line.len() <= 76,
                "a line with an encoded-word holds at most 76"
            );
        }
    }
    let fields = fields(&lines);
    let parts = match fields
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("Content-Type"))
        .map(|(_, value)| value.as_str())
        .expect("a message names its type")
    {
        multipart if multipart.starts_with("multipart/") => alternatives(multipart, body),
        _ => vec![part(&fields, body)],
    };
    Read { fields, parts }
}

fn fields(lines: &[&str]) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in lines {
        if line.starts_with([' ', '\t']) {
            assert!(
                !line.trim().is_empty(),
                "a folded line holds more than whitespace"
            );
            let (_, value) = fields.last_mut().expect("a folded line follows a field");
            value.push_str(line);
        } else {
            let (name, value) = line.split_once(':').expect("a field names itself");
            assert!(!name.is_empty(), "a field's name is not empty");
            assert!(
                name.bytes()
                    .all(|byte| (b'!'..=b'~').contains(&byte) && byte != b':'),
                "a field's name is printable ASCII"
            );
            fields.push((name.to_owned(), value.to_owned()));
        }
    }
    fields
        .into_iter()
        .map(|(name, value)| (name, value.trim_start_matches([' ', '\t']).to_owned()))
        .collect()
}

fn alternatives(content_type: &str, body: &str) -> Vec<(String, String)> {
    let boundary = content_type
        .split_once("boundary=\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(boundary, _)| boundary)
        .expect("a multipart type names its boundary");
    assert!(
        (1..=70).contains(&boundary.len()),
        "a boundary holds 1 to 70 characters"
    );
    let delimiter = format!("--{boundary}\r\n");
    let close = format!("\r\n--{boundary}--\r\n");
    let inside = body
        .strip_prefix(delimiter.as_str())
        .expect("the body starts at its first delimiter");
    let inside = inside
        .strip_suffix(close.as_str())
        .expect("the body ends at its close delimiter");
    inside
        .split(&format!("\r\n{delimiter}"))
        .map(|part| {
            assert!(
                !part.contains(&format!("--{boundary}")),
                "a part holds no delimiter"
            );
            let (head, body) = part.split_once("\r\n\r\n").expect("a part's header ends");
            let lines: Vec<&str> = head.split("\r\n").collect();
            let fields = fields(&lines);
            self::part(&fields, body)
        })
        .collect()
}

fn part(fields: &[(String, String)], body: &str) -> (String, String) {
    let field = |wanted: &str| {
        fields
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
            .map_or_else(
                || panic!("a part names its {wanted}"),
                |(_, value)| value.clone(),
            )
    };
    let media = field("Content-Type");
    assert_eq!(field("Content-Transfer-Encoding"), "quoted-printable");
    let (media, charset) = media
        .split_once("; ")
        .expect("a text part names its charset");
    assert_eq!(charset, "charset=utf-8");
    (media.to_owned(), quoted_printable(body))
}

/// `body` decoded from quoted-printable (RFC 2045 §6.7).
pub fn quoted_printable(body: &str) -> String {
    let body = body
        .strip_suffix("\r\n")
        .expect("a body ends its last line");
    let mut bytes = Vec::new();
    for line in body.split("\r\n") {
        assert!(
            line.len() <= 76,
            "an encoded line holds at most 76 characters"
        );
        let (line, soft) = match line.strip_suffix('=') {
            Some(line) => (line, true),
            None => (line, false),
        };
        if !soft {
            assert!(
                !line.ends_with([' ', '\t']),
                "no whitespace ends a line as itself"
            );
        }
        let mut rest = line.as_bytes();
        while let Some((&byte, after)) = rest.split_first() {
            if byte == b'=' {
                let hex = after.get(..2).expect("= is followed by two digits");
                let hex = std::str::from_utf8(hex).expect("two digits");
                assert!(
                    hex.bytes()
                        .all(|digit| matches!(digit, b'0'..=b'9' | b'A'..=b'F')),
                    "= is followed by two upper-case hexadecimal digits"
                );
                bytes.push(u8::from_str_radix(hex, 16).expect("hexadecimal"));
                rest = &after[2..];
            } else {
                bytes.push(byte);
                rest = after;
            }
        }
        if !soft {
            bytes.extend_from_slice(b"\r\n");
        }
    }
    String::from_utf8(bytes).expect("the body is UTF-8")
}

/// `value` as unstructured text (RFC 5322 §3.2.5), its encoded-words decoded
/// and the whitespace between two of them dropped (RFC 2047 §6.2).
pub fn unstructured(value: &str) -> String {
    let mut text = String::new();
    let mut after_word = false;
    for (at, token) in value.split(' ').enumerate() {
        assert!(
            value.is_empty() || !token.is_empty(),
            "one space between tokens"
        );
        let word = decoded(token);
        if at > 0 && !(after_word && word.is_some()) {
            text.push(' ');
        }
        after_word = word.is_some();
        text.push_str(word.as_deref().unwrap_or(token));
    }
    text
}

/// `token` decoded where it is an encoded-word (RFC 2047 §2, §4.2).
pub fn decoded(token: &str) -> Option<String> {
    let text = token.strip_prefix("=?")?.strip_suffix("?=")?;
    let (charset, rest) = text.split_once('?')?;
    let (encoding, encoded) = rest.split_once('?')?;
    assert!(
        token.len() <= 75,
        "an encoded-word holds at most 75 characters"
    );
    assert_eq!(charset, "UTF-8");
    assert_eq!(encoding, "Q");
    assert!(
        encoded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!*+-/=_".contains(&byte)),
        "Q text in a phrase holds only what RFC 2047 §5(3) allows"
    );
    let mut bytes = Vec::new();
    let mut rest = encoded.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        match byte {
            b'=' => {
                let hex = std::str::from_utf8(after.get(..2).expect("two digits")).expect("ASCII");
                bytes.push(u8::from_str_radix(hex, 16).expect("hexadecimal"));
                rest = &after[2..];
            }
            b'_' => {
                bytes.push(b' ');
                rest = after;
            }
            _ => {
                bytes.push(byte);
                rest = after;
            }
        }
    }
    Some(String::from_utf8(bytes).expect("each encoded-word holds whole characters"))
}

/// `value` as an address list (RFC 5322 §3.4): each mailbox's display name,
/// from one quoted string or a run of encoded-words, and its address.
pub fn mailboxes(value: &str) -> Vec<Seen> {
    let mut seen = Vec::new();
    let mut name: Option<String> = None;
    let mut rest = value;
    loop {
        rest = rest.trim_start_matches(' ');
        if rest.is_empty() {
            assert!(name.is_none(), "a name is followed by its address");
            return seen;
        }
        if let Some(quoted) = rest.strip_prefix('"') {
            let (quoted, after) = quoted.split_once('"').expect("a quoted string ends");
            assert!(!quoted.contains('\\'), "a name needs no escape");
            assert!(name.is_none(), "a name is one quoted string");
            name = Some(quoted.to_owned());
            rest = after;
        } else if rest.starts_with("=?") {
            let end = rest.find(' ').unwrap_or(rest.len());
            let word = decoded(&rest[..end]).expect("an encoded-word");
            name.get_or_insert_with(String::new).push_str(&word);
            rest = &rest[end..];
        } else {
            let angled = rest
                .strip_prefix('<')
                .expect("an address stands between angle brackets");
            let (address, after) = angled.split_once('>').expect("an angle address ends");
            assert!(
                !address.is_empty()
                    && address
                        .bytes()
                        .all(|byte| (b'!'..=b'~').contains(&byte)
                            && !b"<>,\"()[]\\;:".contains(&byte)),
                "an address holds no space, quote or delimiter"
            );
            seen.push((name.take(), address.to_owned()));
            rest = after.strip_prefix(',').unwrap_or_else(|| {
                assert!(
                    after.trim_start().is_empty(),
                    "mailboxes are joined by commas"
                );
                after
            });
        }
    }
}

/// `value` as one message id between angle brackets.
pub fn message_id(value: &str) -> &str {
    value
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .expect("a message id stands between angle brackets")
}

/// `text` as its lines read back: each ended by CRLF, CR LF, LF or CR
/// written as CRLF, the last ended where the text does not end one.
pub fn as_lines(text: &str) -> String {
    let mut text = text.replace("\r\n", "\n").replace('\r', "\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.replace('\n', "\r\n")
}
