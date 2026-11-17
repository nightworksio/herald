//! HTTP message signatures (RFC 9421) by Ed25519: the signature base a
//! request or response is signed over, the fields the signature travels
//! in, and what a receiver demands of a signature it is shown.

use std::fmt::{self, Write as _};

use herald_core::Address;

use crate::structured::{Bare, Item, Member, dictionary, written, written_inner};

/// The field naming what a signature covers.
pub const SIGNATURE_INPUT: &str = "signature-input";
/// The field carrying the signature.
pub const SIGNATURE: &str = "signature";
/// The label herald's signature takes in both fields.
pub const LABEL: &str = "herald";
/// The tag herald's signature carries, by which a receiver finds it among
/// others.
const TAG: &str = "herald";
/// The algorithm herald signs with, as RFC 9421 names it.
const ED25519: &str = "ed25519";
/// The most bytes a signature field holds where the receiver names no
/// other bound.
pub const FIELD_MOST: usize = 8192;
/// The name of the base's last line.
const PARAMS: &str = "@signature-params";
/// The parameter naming when a signature was made.
const CREATED: &str = "created";
/// The parameter naming when a signature expires.
const EXPIRES: &str = "expires";
/// The parameter carrying a signature's nonce.
const NONCE: &str = "nonce";
/// The parameter naming the signing key.
const KEYID: &str = "keyid";
/// The parameter naming the algorithm.
const ALG: &str = "alg";
/// The parameter carrying the application's tag.
const TAG_PARAMETER: &str = "tag";

/// One part of a message a signature covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Component {
    /// `@method`: the request's method.
    Method,
    /// `@target-uri`: the request's full URL.
    TargetUri,
    /// `@authority`: the request's host, and port where not the scheme's.
    Authority,
    /// `@scheme`: the request's scheme.
    Scheme,
    /// `@path`: the request's path.
    Path,
    /// `@query`: the request's query, after a `?`.
    Query,
    /// `@status`: the response's status code.
    Status,
    /// A field by its lowercase name.
    Field(String),
}

impl Component {
    /// Its name in a signature's base and input.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Method => "@method",
            Self::TargetUri => "@target-uri",
            Self::Authority => "@authority",
            Self::Scheme => "@scheme",
            Self::Path => "@path",
            Self::Query => "@query",
            Self::Status => "@status",
            Self::Field(name) => name,
        }
    }

    fn named(name: &str) -> Option<Self> {
        let derived = [
            Self::Method,
            Self::TargetUri,
            Self::Authority,
            Self::Scheme,
            Self::Path,
            Self::Query,
            Self::Status,
        ];
        if name.starts_with('@') {
            return derived
                .into_iter()
                .find(|component| component.name() == name);
        }
        let field = !name.is_empty()
            && name.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || b"!#$%&'*+-.^_`|~".contains(&byte)
            });
        field.then(|| Self::Field(name.to_owned()))
    }
}

/// The message a signature is made or checked over: a request, or a
/// response with the request it answers.
#[derive(Debug, Clone, Copy)]
pub struct Message<'a> {
    /// The request's method.
    pub method: &'a str,
    /// The request's full URL.
    pub url: &'a str,
    /// The response's status code, for a response.
    pub status: Option<u16>,
    /// The message's fields, in order, each name in any case.
    pub fields: &'a [(String, String)],
}

impl Message<'_> {
    fn value(&self, component: &Component) -> Result<String, Unsigned> {
        let missing = || Unsigned::Missing {
            component: component.name().to_owned(),
        };
        let address = || {
            Address::parse(self.url).ok_or_else(|| Unsigned::Address {
                url: self.url.to_owned(),
            })
        };
        let value = match component {
            Component::Method => self.method.to_owned(),
            Component::TargetUri => {
                let address = address()?;
                format!("{}{}", address.origin(), address.target())
            }
            Component::Authority => address()?.authority(),
            Component::Scheme => address()?.scheme().to_owned(),
            Component::Path => {
                let address = address()?;
                let (path, _) = split_target(address.target());
                path.to_owned()
            }
            Component::Query => {
                let address = address()?;
                let (_, query) = split_target(address.target());
                format!("?{}", query.unwrap_or_default())
            }
            Component::Status => self.status.ok_or_else(missing)?.to_string(),
            Component::Field(name) => {
                let values: Vec<&str> = self
                    .fields
                    .iter()
                    .filter(|(field, _)| field.eq_ignore_ascii_case(name))
                    .map(|(_, value)| value.trim_matches([' ', '\t']))
                    .collect();
                if values.is_empty() {
                    return Err(missing());
                }
                values.join(", ")
            }
        };
        if value.contains(['\r', '\n']) {
            return Err(Unsigned::Broken {
                component: component.name().to_owned(),
            });
        }
        Ok(value)
    }
}

fn split_target(target: &str) -> (&str, Option<&str>) {
    match target.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (target, None),
    }
}

/// Why a signature base could not be made of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unsigned {
    /// The message lacks a part the signature covers.
    Missing {
        /// The part's name.
        component: String,
    },
    /// A part's value holds a line break, which would forge a line of the
    /// base.
    Broken {
        /// The part's name.
        component: String,
    },
    /// The message's URL names no host.
    Address {
        /// The URL.
        url: String,
    },
}

impl fmt::Display for Unsigned {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { component } => {
                write!(
                    f,
                    "the message has no {component}, which the signature covers"
                )
            }
            Self::Broken { component } => write!(
                f,
                "the message's {component} holds a line break, so it cannot be signed"
            ),
            Self::Address { url } => write!(f, "the message's URL {url} names no host"),
        }
    }
}

/// What one signature covers: the parts of the message, when it was made,
/// when it expires, a nonce where it changes anything, and the key's id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Covered {
    /// The parts of the message, in order.
    pub components: Vec<Component>,
    /// When it was made, in seconds since the Unix epoch.
    pub created: i64,
    /// When it expires, in seconds since the Unix epoch.
    pub expires: i64,
    /// A value never used twice, for a request that changes anything.
    pub nonce: Option<String>,
    /// The id of the key that signs it.
    pub keyid: String,
}

impl Covered {
    /// The `@signature-params` value: what [`SIGNATURE_INPUT`] carries
    /// under the label.
    #[must_use]
    pub fn params(&self) -> String {
        let mut parameters = vec![
            (CREATED.to_owned(), Bare::Integer(self.created)),
            (EXPIRES.to_owned(), Bare::Integer(self.expires)),
        ];
        if let Some(nonce) = &self.nonce {
            parameters.push((NONCE.to_owned(), Bare::String(nonce.clone())));
        }
        parameters.extend([
            (KEYID.to_owned(), Bare::String(self.keyid.clone())),
            (ALG.to_owned(), Bare::String(ED25519.to_owned())),
            (TAG_PARAMETER.to_owned(), Bare::String(TAG.to_owned())),
        ]);
        listed(&self.components, &parameters)
    }

    /// The signature base of `message`: the bytes the key signs.
    ///
    /// # Errors
    ///
    /// The message lacks a covered part, a part holds a line break, or its
    /// URL names no host.
    pub fn base(&self, message: &Message<'_>) -> Result<Vec<u8>, Unsigned> {
        signature_base(&self.components, message, &self.params())
    }

    /// The two fields `signature`, made over this, travels in.
    #[must_use]
    pub fn fields(&self, signature: &[u8]) -> [(String, String); 2] {
        [
            (
                SIGNATURE_INPUT.to_owned(),
                format!("{LABEL}={}", self.params()),
            ),
            (
                SIGNATURE.to_owned(),
                format!("{LABEL}={}", written(&Bare::Bytes(signature.to_vec()))),
            ),
        ]
    }
}

/// The inner list of `components` with `parameters`: a signature's
/// `@signature-params`.
fn listed(components: &[Component], parameters: &[(String, Bare)]) -> String {
    let names: Vec<Bare> = components
        .iter()
        .map(|component| Bare::String(component.name().to_owned()))
        .collect();
    written_inner(&names, parameters)
}

/// The signature base of `message` over `components`, ending in `params`.
fn signature_base(
    components: &[Component],
    message: &Message<'_>,
    params: &str,
) -> Result<Vec<u8>, Unsigned> {
    let mut base = String::new();
    for component in components {
        let _ = writeln!(
            base,
            "\"{}\": {}",
            component.name(),
            message.value(component)?
        );
    }
    let _ = write!(base, "\"{PARAMS}\": {params}");
    Ok(base.into_bytes())
}

/// Why a signature shown with a message is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The message carries no signature tagged as herald's.
    Unsigned,
    /// The signature's fields cannot be read: what is wrong.
    Unreadable(&'static str),
    /// The signature names an algorithm other than Ed25519.
    Algorithm(String),
    /// The signature leaves out a part the receiver demands.
    Uncovered(String),
    /// The signature was made later than now, past the allowed skew.
    Early,
    /// The signature has expired.
    Expired,
    /// The signature lives longer than the receiver allows.
    Lasting,
    /// The signature carries no nonce where the receiver demands one.
    Unnonced,
    /// No signature base can be made of the message.
    Base(Unsigned),
    /// A signature field is longer than the receiver reads.
    Oversized {
        /// The field.
        field: &'static str,
        /// The most bytes it may hold.
        most: usize,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsigned => write!(f, "the message carries no signature tagged {TAG}"),
            Self::Unreadable(what) => write!(f, "the signature cannot be read: {what}"),
            Self::Algorithm(alg) => write!(f, "the signature is by {alg}, not {ED25519}"),
            Self::Uncovered(name) => write!(f, "the signature does not cover {name}"),
            Self::Early => write!(f, "the signature was made later than now"),
            Self::Expired => write!(f, "the signature has expired"),
            Self::Lasting => write!(f, "the signature lasts longer than allowed"),
            Self::Unnonced => write!(f, "the signature carries no nonce"),
            Self::Base(unsigned) => unsigned.fmt(f),
            Self::Oversized { field, most } => {
                write!(f, "the {field} field holds more than the {most} bytes read")
            }
        }
    }
}

/// A signature shown with a message: what it covers, its bytes, and the
/// base it must be the signature of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presented {
    /// What it covers.
    pub covered: Covered,
    /// Its bytes.
    pub signature: Vec<u8>,
    /// The signature base of the message it covers.
    pub base: Vec<u8>,
}

/// What a receiver demands of a signature, at a time.
#[derive(Debug, Clone, Copy)]
pub struct Demands<'a> {
    /// The parts it must cover.
    pub components: &'a [Component],
    /// Whether it must be single-use: carry a nonce.
    pub single_use: bool,
    /// Now, in seconds since the Unix epoch.
    pub now: i64,
    /// How many seconds its creation may lie ahead of now, or its expiry
    /// behind.
    pub skew: i64,
    /// The most seconds it may live, from its creation to its expiry.
    pub longest: i64,
}

/// The signature tagged as herald's that `message` carries, with the base
/// it must be the signature of; each signature field is read only where it
/// holds at most `most` bytes.
///
/// # Errors
///
/// The message carries no such signature, its fields are too long or
/// cannot be read, it names another algorithm, or no base can be made of
/// the message.
pub fn presented(message: &Message<'_>, most: usize) -> Result<Presented, Refusal> {
    let inputs = dictionary(&signature_field(message, SIGNATURE_INPUT, most)?)
        .ok_or(Refusal::Unreadable("Signature-Input is no dictionary"))?;
    let (label, items, parameters) = inputs
        .into_iter()
        .find_map(|(label, member)| match member {
            Member::Inner(items, parameters)
                if parameters
                    .contains(&(TAG_PARAMETER.to_owned(), Bare::String(TAG.to_owned()))) =>
            {
                Some((label, items, parameters))
            }
            _ => None,
        })
        .ok_or(Refusal::Unsigned)?;
    let covered = covered(items, &parameters)?;
    let signature = signature_under(message, &label, most)?;
    let base = signature_base(
        &covered.components,
        message,
        &listed(&covered.components, &parameters),
    )
    .map_err(Refusal::Base)?;
    Ok(Presented {
        covered,
        signature,
        base,
    })
}

/// The value of the signature field `name` of `message`, where it holds at
/// most `most` bytes.
fn signature_field(
    message: &Message<'_>,
    name: &'static str,
    most: usize,
) -> Result<String, Refusal> {
    let value = message
        .value(&Component::Field(name.to_owned()))
        .map_err(|why| match why {
            Unsigned::Missing { .. } => Refusal::Unsigned,
            _ => Refusal::Unreadable("a signature field holds a line break"),
        })?;
    if value.len() > most {
        return Err(Refusal::Oversized { field: name, most });
    }
    Ok(value)
}

/// What a signature input's `items` and `parameters` say it covers.
fn covered(items: Vec<Item>, parameters: &[(String, Bare)]) -> Result<Covered, Refusal> {
    let components = items
        .into_iter()
        .map(|item| match (item.bare, item.parameters.is_empty()) {
            (Bare::String(name), true) => Component::named(&name),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(Refusal::Unreadable(
            "a covered part is not one herald reads",
        ))?;
    let integer = |name: &str| {
        parameters.iter().find_map(|(key, value)| match value {
            Bare::Integer(number) if key == name => Some(*number),
            _ => None,
        })
    };
    let string = |name: &str| {
        parameters.iter().find_map(|(key, value)| match value {
            Bare::String(text) if key == name => Some(text.clone()),
            _ => None,
        })
    };
    if let Some(alg) = string(ALG).filter(|alg| alg != ED25519) {
        return Err(Refusal::Algorithm(alg));
    }
    Ok(Covered {
        components,
        created: integer(CREATED).ok_or(Refusal::Unreadable("it names no creation time"))?,
        expires: integer(EXPIRES).ok_or(Refusal::Unreadable("it names no expiry"))?,
        nonce: string(NONCE),
        keyid: string(KEYID).ok_or(Refusal::Unreadable("it names no key"))?,
    })
}

/// The bytes the `Signature` field of `message` carries under `label`.
fn signature_under(message: &Message<'_>, label: &str, most: usize) -> Result<Vec<u8>, Refusal> {
    dictionary(&signature_field(message, SIGNATURE, most)?)
        .ok_or(Refusal::Unreadable("Signature is no dictionary"))?
        .into_iter()
        .find_map(|(held, member)| match member {
            Member::Item(Item {
                bare: Bare::Bytes(bytes),
                ..
            }) if held == label => Some(bytes),
            _ => None,
        })
        .ok_or(Refusal::Unreadable(
            "Signature carries no bytes under the label",
        ))
}

impl Presented {
    /// Whether it meets `demands`: it covers every part they name, carries
    /// a nonce where they demand one, and is live now, within the skew, for
    /// no longer than they allow.
    ///
    /// # Errors
    ///
    /// The first demand it fails.
    pub fn meets(&self, demands: Demands<'_>) -> Result<(), Refusal> {
        let covered = &self.covered;
        if let Some(left_out) = demands
            .components
            .iter()
            .find(|component| !covered.components.contains(component))
        {
            return Err(Refusal::Uncovered(left_out.name().to_owned()));
        }
        if demands.single_use && covered.nonce.is_none() {
            return Err(Refusal::Unnonced);
        }
        if covered.created > demands.now.saturating_add(demands.skew) {
            return Err(Refusal::Early);
        }
        if covered.expires < demands.now.saturating_sub(demands.skew) {
            return Err(Refusal::Expired);
        }
        if covered.expires.saturating_sub(covered.created) > demands.longest {
            return Err(Refusal::Lasting);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
