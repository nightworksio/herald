use std::fmt::Write as _;

use crate::address::Mailbox;

/// The most characters of a header line herald writes where it chooses where
/// lines end (RFC 5322 §2.1.1: at most 998, and no more than 78 should).
pub const LINE: usize = 78;
/// The most characters of a header line holding an encoded-word
/// (RFC 2047 §2).
pub const ENCODED_LINE: usize = 76;
/// The most characters of one encoded-word (RFC 2047 §2).
pub const ENCODED_WORD: usize = 75;
/// The most characters one character takes in Q-encoded text: four bytes of
/// UTF-8, each written `=XX`.
const ENCODED_CHARACTER: usize = 12;

const OPEN: &str = "=?UTF-8?Q?";
const CLOSE: &str = "?=";
/// The characters Q-encoded text holds as themselves: those RFC 2047 §5(3)
/// allows in a phrase, which §5(1) allows in unstructured text too, less `=`
/// and `_`, which Q encoding gives a meaning of their own (§4.2).
const AS_THEMSELVES: &[u8] = b"!*+-/";

/// A message's header block, each field written whole.
#[derive(Debug, Default)]
pub(crate) struct Head {
    text: String,
}

impl Head {
    /// A field named `name`, written until the field ends.
    pub(crate) fn field(&mut self, name: &'static str) -> Field<'_> {
        self.text.push_str(name);
        self.text.push(':');
        Field {
            width: name.len() + 1,
            encoded: false,
            text: &mut self.text,
        }
    }

    /// The header block, its last field ended.
    pub(crate) fn into_text(self) -> String {
        self.text
    }
}

/// One header field being written: tokens joined by single spaces, a line
/// folded before a token that would make it longer than RFC 5322 and
/// RFC 2047 allow.
#[derive(Debug)]
pub(crate) struct Field<'head> {
    text: &'head mut String,
    width: usize,
    encoded: bool,
}

impl Field<'_> {
    /// `token` whole, after a space or a fold.
    pub(crate) fn token(&mut self, token: &str) {
        let most = if self.encoded { ENCODED_LINE } else { LINE };
        if self.width + 1 + token.len() > most {
            self.fold();
        }
        self.text.push(' ');
        self.text.push_str(token);
        self.width += 1 + token.len();
    }

    /// `text` as encoded-words, each sized to the line it starts on.
    pub(crate) fn encoded(&mut self, text: &str) {
        let mut characters = text.chars().peekable();
        while let Some(first) = characters.next() {
            if self.width + 1 + OPEN.len() + ENCODED_CHARACTER + CLOSE.len() > ENCODED_LINE {
                self.fold();
            }
            let room = ENCODED_WORD.min(ENCODED_LINE - self.width - 1);
            let mut word = String::from(OPEN);
            word.push_str(&q_encoded(first));
            while let Some(&character) = characters.peek() {
                let written = q_encoded(character);
                if word.len() + written.len() + CLOSE.len() > room {
                    break;
                }
                word.push_str(&written);
                characters.next();
            }
            word.push_str(CLOSE);
            self.encoded = true;
            self.token(&word);
        }
    }

    /// `text` as unstructured text (RFC 5322 §3.2.5): its words as they are
    /// where it is printable ASCII that reads as nothing but itself and fits
    /// the lines, else as encoded-words.
    pub(crate) fn unstructured(&mut self, text: &str) {
        let words: Vec<&str> = text.split(' ').collect();
        if plain(text)
            && words
                .iter()
                .all(|word| !word.is_empty() && word.len() < LINE)
        {
            for word in words {
                self.token(word);
            }
        } else {
            self.encoded(text);
        }
    }

    /// `mailbox` as a name-addr (RFC 5322 §3.4), its display name where it
    /// has one and its address always between angle brackets, so no reader
    /// takes an address for an encoded-word.
    pub(crate) fn mailbox(&mut self, mailbox: &Mailbox, last: bool) {
        if let Some(name) = mailbox.name() {
            self.phrase(name);
        }
        let comma = if last { "" } else { "," };
        self.token(&format!("<{}>{comma}", mailbox.address()));
    }

    /// `name` as a phrase: a quoted string where it is printable ASCII that
    /// reads as nothing but itself and fits a line, else encoded-words
    /// (RFC 2047 §5(3)). A name holds no quote or backslash
    /// ([`Mailbox::new`]), so the quoted string needs no escape.
    fn phrase(&mut self, name: &str) {
        let quoted = format!("\"{name}\"");
        if plain(name) && quoted.len() < LINE {
            self.token(&quoted);
        } else {
            self.encoded(name);
        }
    }

    fn fold(&mut self) {
        self.text.push_str("\r\n");
        self.width = 0;
        self.encoded = false;
    }
}

impl Drop for Field<'_> {
    fn drop(&mut self) {
        self.text.push_str("\r\n");
    }
}

/// Whether `text` is printable ASCII holding nothing a reader decodes as an
/// encoded-word (RFC 2047 §5: "it may be necessary to encode ordinary text
/// that looks like an encoded-word").
fn plain(text: &str) -> bool {
    text.bytes().all(|byte| (b' '..=b'~').contains(&byte)) && !text.contains("=?")
}

/// `character` as Q-encoded text (RFC 2047 §4.2).
fn q_encoded(character: char) -> String {
    let mut written = String::new();
    let mut bytes = [0; 4];
    for &byte in character.encode_utf8(&mut bytes).as_bytes() {
        if byte.is_ascii_alphanumeric() || AS_THEMSELVES.contains(&byte) {
            written.push(char::from(byte));
        } else if byte == b' ' {
            written.push('_');
        } else {
            let _ = write!(written, "={byte:02X}");
        }
    }
    written
}
