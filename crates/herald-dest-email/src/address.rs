use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, de};

/// The most bytes of a local part (RFC 5321 §4.5.3.1.1).
pub const LOCAL_MOST: usize = 64;
/// The most bytes of a domain's label (RFC 1035 §2.3.4).
pub const LABEL_MOST: usize = 63;
/// The most bytes of an address: a path of at most 256 bytes less its angle
/// brackets (RFC 5321 §4.5.3.1.3).
pub const ADDRESS_MOST: usize = 254;
/// The most bytes of a display name.
pub const NAME_MOST: usize = 256;

/// The printable ASCII an atom holds besides letters and digits
/// (RFC 5322 §3.2.3, `atext`).
const ATEXT: &str = "!#$%&'*+-/=?^_`{|}~";

/// Why a text is not an address herald writes. No reason repeats the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unparsed {
    /// The text holds nothing.
    Empty,
    /// The text holds a control character, a line break among them.
    Control,
    /// The text has no `@`, or more than one, or text around its angle brackets.
    Unaddressed,
    /// The local part is not a dot-atom of at most [`LOCAL_MOST`] bytes.
    Local,
    /// The domain is not ASCII labels of letters, digits and hyphens.
    Domain,
    /// The address is longer than [`ADDRESS_MOST`] bytes.
    Long,
    /// The display name holds a character herald does not write in one.
    Name,
}

impl fmt::Display for Unparsed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("is empty"),
            Self::Control => f.write_str("holds a control character, such as a line break"),
            Self::Unaddressed => {
                f.write_str("is no address: write it as local@domain or as Name <local@domain>")
            }
            Self::Local => write!(
                f,
                "has a local part herald does not write: letters, digits and {ATEXT} in runs \
                 joined by single dots, at most {LOCAL_MOST} bytes"
            ),
            Self::Domain => write!(
                f,
                "has a domain herald does not write: labels of ASCII letters, digits and \
                 hyphens joined by single dots, each at most {LABEL_MOST} bytes and starting \
                 and ending with a letter or digit; write an international domain in its xn-- \
                 form"
            ),
            Self::Long => write!(
                f,
                "is longer than the {ADDRESS_MOST} bytes an address holds"
            ),
            Self::Name => write!(
                f,
                "has a display name herald does not write: at most {NAME_MOST} bytes, with no \
                 control character, line separator, quote, backslash, < or >"
            ),
        }
    }
}

impl std::error::Error for Unparsed {}

/// An address, `local@domain`, as herald writes it in a header and an
/// envelope: a dot-atom local part (RFC 5322 §3.4.1), never the quoted form
/// RFC 5321 §4.1.2 says to avoid, and an ASCII domain of host-name labels. It
/// holds no space, control character, comma, quote or angle bracket, so it
/// can be written whole wherever an address stands.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AddrSpec {
    local: String,
    domain: String,
}

impl AddrSpec {
    /// The address `text` writes, where it is one herald writes.
    ///
    /// # Errors
    ///
    /// Why `text` is not such an address.
    pub fn parse(text: &str) -> Result<Self, Unparsed> {
        if text.is_empty() {
            return Err(Unparsed::Empty);
        }
        if text.len() > ADDRESS_MOST {
            return Err(Unparsed::Long);
        }
        let (local, domain) = text.rsplit_once('@').ok_or(Unparsed::Unaddressed)?;
        if !dot_atom(local) || local.len() > LOCAL_MOST {
            return Err(Unparsed::Local);
        }
        if !host_name(domain) {
            return Err(Unparsed::Domain);
        }
        Ok(Self {
            local: local.to_owned(),
            domain: domain.to_owned(),
        })
    }

    /// The part before the `@`.
    #[must_use]
    pub fn local(&self) -> &str {
        &self.local
    }

    /// The part after the `@`.
    #[must_use]
    pub fn domain(&self) -> &str {
        &self.domain
    }

    /// Whether `other` names the same mailbox: the local part as written,
    /// the domain in any case (RFC 5321 §2.4).
    #[must_use]
    pub fn same(&self, other: &Self) -> bool {
        self.local == other.local && self.domain.eq_ignore_ascii_case(&other.domain)
    }
}

impl fmt::Display for AddrSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.local, self.domain)
    }
}

/// Whether `text` is a dot-atom: runs of `atext` joined by single dots
/// (RFC 5322 §3.2.3).
fn dot_atom(text: &str) -> bool {
    text.split('.').all(|run| {
        !run.is_empty()
            && run
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || ATEXT.as_bytes().contains(&byte))
    })
}

/// Whether `text` is a host name: labels of letters, digits and hyphens, none
/// starting or ending with a hyphen, joined by single dots (RFC 1123 §2.1,
/// RFC 5321 §4.1.2 `Domain`). An address of at most [`ADDRESS_MOST`] bytes
/// holds a domain shorter than the 255 bytes RFC 5321 §4.5.3.1.2 allows.
fn host_name(text: &str) -> bool {
    text.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= LABEL_MOST
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

/// A mailbox: an address and the display name the config gives it, if any.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Mailbox {
    name: Option<String>,
    address: AddrSpec,
}

impl Mailbox {
    /// `address` shown as `name`, where `name` is one herald writes; a name of
    /// whitespace alone shows none.
    ///
    /// # Errors
    ///
    /// [`Unparsed::Name`] where `name` is not one herald writes.
    pub fn new(name: Option<&str>, address: AddrSpec) -> Result<Self, Unparsed> {
        let name = match name.map(str::trim) {
            Some(name) if !name.is_empty() => {
                if !shown(name) {
                    return Err(Unparsed::Name);
                }
                Some(name.to_owned())
            }
            _ => None,
        };
        Ok(Self { name, address })
    }

    /// The mailbox `text` writes, `local@domain` or `Name <local@domain>`,
    /// the name in double quotes or not.
    ///
    /// # Errors
    ///
    /// Why `text` is not a mailbox herald writes.
    pub fn parse(text: &str) -> Result<Self, Unparsed> {
        if text.contains(char::is_control) {
            return Err(Unparsed::Control);
        }
        let text = text.trim();
        if text.is_empty() {
            return Err(Unparsed::Empty);
        }
        let Some(angled) = text.strip_suffix('>') else {
            if text.contains(['<', '>']) || text.contains(char::is_whitespace) {
                return Err(Unparsed::Unaddressed);
            }
            return Self::new(None, AddrSpec::parse(text)?);
        };
        let (name, address) = angled.rsplit_once('<').ok_or(Unparsed::Unaddressed)?;
        let name = name.trim();
        let name = name
            .strip_prefix('"')
            .and_then(|name| name.strip_suffix('"'))
            .unwrap_or(name);
        Self::new(Some(name), AddrSpec::parse(address)?)
    }

    /// The display name, where the config gives one.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The address.
    #[must_use]
    pub fn address(&self) -> &AddrSpec {
        &self.address
    }

    /// The mailbox as the config writes it, which [`Self::parse`] reads
    /// back: `Name <local@domain>`, or its address alone.
    #[must_use]
    pub fn written(&self) -> String {
        match &self.name {
            Some(name) => format!("{name} <{}>", self.address),
            None => self.address.to_string(),
        }
    }

    /// The mailbox `text` writes, for a reader of the config: why it is
    /// none, without repeating it, otherwise.
    pub(crate) fn read<E: de::Error>(text: &str) -> Result<Self, E> {
        Self::parse(text).map_err(|why| E::custom(format!("the address {why}")))
    }
}

/// Whether herald writes `name` as a display name: at most [`NAME_MOST`]
/// bytes, and no character that would end a line, close a quoted string or
/// stand for an address.
fn shown(name: &str) -> bool {
    name.len() <= NAME_MOST
        && !name.chars().any(|character| {
            character.is_control()
                || matches!(character, '\u{2028}' | '\u{2029}' | '"' | '\\' | '<' | '>')
        })
}

impl<'de> Deserialize<'de> for Mailbox {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::read(&String::deserialize(deserializer)?)
    }
}

impl JsonSchema for Mailbox {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Mailbox".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "An address, written local@domain or Name <local@domain>.",
            "type": "string",
            "minLength": 3
        })
    }
}
