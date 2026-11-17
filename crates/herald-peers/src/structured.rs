//! The structured field values (RFC 8941) HTTP message signatures and
//! content digests travel in: a dictionary of items and inner lists, each
//! with parameters.

use std::fmt::Write as _;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

/// One bare item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bare {
    /// An integer.
    Integer(i64),
    /// A string of printable ASCII.
    String(String),
    /// A token.
    Token(String),
    /// A byte sequence.
    Bytes(Vec<u8>),
    /// A boolean.
    Boolean(bool),
}

/// An item's or an inner list's parameters, in order.
pub type Parameters = Vec<(String, Bare)>;

/// One item with its parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The value.
    pub bare: Bare,
    /// The parameters.
    pub parameters: Parameters,
}

/// One dictionary member's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Member {
    /// An item.
    Item(Item),
    /// An inner list of items, with the list's parameters.
    Inner(Vec<Item>, Parameters),
}

/// The most digits an integer is written with.
const DIGITS: usize = 15;

/// The members of the dictionary `text`, in order, a later member of a
/// name replacing an earlier one; `None` where `text` is no dictionary.
#[must_use]
pub fn dictionary(text: &str) -> Option<Vec<(String, Member)>> {
    let mut reader = Reader {
        rest: text.trim_matches(' ').as_bytes(),
    };
    let mut members: Vec<(String, Member)> = Vec::new();
    while !reader.rest.is_empty() {
        let name = reader.key()?;
        let member = if reader.eat(b'=') {
            reader.member()?
        } else {
            Member::Item(Item {
                bare: Bare::Boolean(true),
                parameters: reader.parameters()?,
            })
        };
        members.retain(|(held, _)| *held != name);
        members.push((name, member));
        reader.skip_whitespace();
        if reader.rest.is_empty() {
            break;
        }
        if !reader.eat(b',') {
            return None;
        }
        reader.skip_whitespace();
        if reader.rest.is_empty() {
            return None;
        }
    }
    Some(members)
}

struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    fn peek(&self) -> Option<u8> {
        self.rest.first().copied()
    }

    fn eat(&mut self, byte: u8) -> bool {
        let found = self.peek() == Some(byte);
        if found {
            self.advance(1);
        }
        found
    }

    fn advance(&mut self, count: usize) {
        self.rest = self.rest.get(count..).unwrap_or_default();
    }

    fn take_while(&mut self, keep: impl Fn(u8) -> bool) -> &'a [u8] {
        let length = self.rest.iter().take_while(|byte| keep(**byte)).count();
        let (taken, rest) = self.rest.split_at(length);
        self.rest = rest;
        taken
    }

    fn skip_whitespace(&mut self) {
        self.take_while(|byte| byte == b' ' || byte == b'\t');
    }

    fn key(&mut self) -> Option<String> {
        let first = self.peek()?;
        if !(first.is_ascii_lowercase() || first == b'*') {
            return None;
        }
        let key = self.take_while(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-.*".contains(&byte)
        });
        Some(String::from_utf8_lossy(key).into_owned())
    }

    fn member(&mut self) -> Option<Member> {
        if !self.eat(b'(') {
            return self.item().map(Member::Item);
        }
        let mut items = Vec::new();
        loop {
            self.take_while(|byte| byte == b' ');
            if self.eat(b')') {
                return Some(Member::Inner(items, self.parameters()?));
            }
            items.push(self.item()?);
            if !matches!(self.peek(), Some(b' ' | b')')) {
                return None;
            }
        }
    }

    fn item(&mut self) -> Option<Item> {
        let bare = self.bare()?;
        Some(Item {
            bare,
            parameters: self.parameters()?,
        })
    }

    fn parameters(&mut self) -> Option<Parameters> {
        let mut parameters: Parameters = Vec::new();
        while self.eat(b';') {
            self.take_while(|byte| byte == b' ');
            let name = self.key()?;
            let value = if self.eat(b'=') {
                self.bare()?
            } else {
                Bare::Boolean(true)
            };
            parameters.retain(|(held, _)| *held != name);
            parameters.push((name, value));
        }
        Some(parameters)
    }

    fn bare(&mut self) -> Option<Bare> {
        match self.peek()? {
            b'"' => self.string(),
            b':' => self.bytes(),
            b'?' => self.boolean(),
            b'-' | b'0'..=b'9' => self.integer(),
            first if first.is_ascii_alphabetic() || first == b'*' => Some(self.token()),
            _ => None,
        }
    }

    fn string(&mut self) -> Option<Bare> {
        self.advance(1);
        let mut text = String::new();
        loop {
            let byte = self.peek()?;
            self.advance(1);
            match byte {
                b'"' => return Some(Bare::String(text)),
                b'\\' => {
                    let escaped = self.peek().filter(|next| *next == b'"' || *next == b'\\')?;
                    self.advance(1);
                    text.push(char::from(escaped));
                }
                b' '..=b'~' => text.push(char::from(byte)),
                _ => return None,
            }
        }
    }

    fn bytes(&mut self) -> Option<Bare> {
        self.advance(1);
        let written = self.take_while(|byte| byte != b':').to_vec();
        if !self.eat(b':') {
            return None;
        }
        STANDARD.decode(written).ok().map(Bare::Bytes)
    }

    fn boolean(&mut self) -> Option<Bare> {
        self.advance(1);
        let value = match self.peek()? {
            b'0' => false,
            b'1' => true,
            _ => return None,
        };
        self.advance(1);
        Some(Bare::Boolean(value))
    }

    fn integer(&mut self) -> Option<Bare> {
        let negative = self.eat(b'-');
        let digits = self.take_while(|byte| byte.is_ascii_digit());
        if digits.is_empty() || digits.len() > DIGITS || self.peek() == Some(b'.') {
            return None;
        }
        let magnitude: i64 = std::str::from_utf8(digits).ok()?.parse().ok()?;
        Some(Bare::Integer(if negative { -magnitude } else { magnitude }))
    }

    fn token(&mut self) -> Bare {
        let token = self.take_while(|byte| {
            byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~:/".contains(&byte)
        });
        Bare::Token(String::from_utf8_lossy(token).into_owned())
    }
}

/// `bare` written as a structured field writes it.
#[must_use]
pub fn written(bare: &Bare) -> String {
    match bare {
        Bare::Integer(number) => number.to_string(),
        Bare::String(text) => {
            let mut out = String::from('"');
            for character in text.chars() {
                if character == '"' || character == '\\' {
                    out.push('\\');
                }
                out.push(character);
            }
            out.push('"');
            out
        }
        Bare::Token(token) => token.clone(),
        Bare::Bytes(bytes) => format!(":{}:", STANDARD.encode(bytes)),
        Bare::Boolean(flag) => if *flag { "?1" } else { "?0" }.to_owned(),
    }
}

/// `parameters` written as a structured field writes them.
#[must_use]
pub fn written_parameters(parameters: &[(String, Bare)]) -> String {
    let mut out = String::new();
    for (name, value) in parameters {
        let _ = write!(out, ";{name}");
        if *value != Bare::Boolean(true) {
            let _ = write!(out, "={}", written(value));
        }
    }
    out
}

/// The inner list of `items`, each a bare item without parameters, with
/// the list's `parameters`.
#[must_use]
pub fn written_inner(items: &[Bare], parameters: &[(String, Bare)]) -> String {
    let listed: Vec<String> = items.iter().map(written).collect();
    format!("({}){}", listed.join(" "), written_parameters(parameters))
}

#[cfg(test)]
mod tests;
