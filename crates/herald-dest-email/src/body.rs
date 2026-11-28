use std::fmt::Write as _;

/// The most characters of a quoted-printable line, its soft line break's `=`
/// among them (RFC 2045 §6.7, rule 5).
pub const QUOTED_LINE: usize = 76;

/// `text` in the quoted-printable encoding (RFC 2045 §6.7): each of its lines,
/// ended by CR LF, LF or CR, written as a line ended by CRLF; every byte but
/// printable ASCII other than `=`, and a space or tab that does not end its
/// line, written `=XX`; a line longer than [`QUOTED_LINE`] broken by soft
/// line breaks. The encoding holds no `=` that is not followed by two
/// hexadecimal digits or a line break, so it never holds `=_`.
pub(crate) fn quoted_printable(text: &str) -> String {
    let mut written = String::new();
    for line in lines(text) {
        let mut width = 0;
        let bytes = line.as_bytes();
        for (at, &byte) in bytes.iter().enumerate() {
            let last = at + 1 == bytes.len();
            let encoded = encoded(byte, last);
            let room = if last { QUOTED_LINE } else { QUOTED_LINE - 1 };
            if width + encoded.len() > room {
                written.push_str("=\r\n");
                width = 0;
            }
            written.push_str(&encoded);
            width += encoded.len();
        }
        written.push_str("\r\n");
    }
    written
}

/// The lines of `text`, each ended by CR LF, LF or CR or by the end of the
/// text; a text ending in a line break ends with the line it ends.
pub(crate) fn lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(text);
    std::iter::from_fn(move || {
        let text = rest?;
        let Some(at) = text.find(['\r', '\n']) else {
            rest = None;
            return Some(text);
        };
        let (line, after) = text.split_at(at);
        let after = after
            .strip_prefix("\r\n")
            .or_else(|| after.get(1..))
            .unwrap_or_default();
        rest = Some(after).filter(|after| !after.is_empty());
        Some(line)
    })
}

fn encoded(byte: u8, last: bool) -> String {
    let literal = match byte {
        b'=' => false,
        b' ' | b'\t' => !last,
        _ => (b'!'..=b'~').contains(&byte),
    };
    if literal {
        char::from(byte).to_string()
    } else {
        let mut written = String::new();
        let _ = write!(written, "={byte:02X}");
        written
    }
}
