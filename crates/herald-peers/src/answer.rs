//! A peer's answer checked before its body is read: signed by a key the
//! reader trusts, over its status, type, body's digest and the URL it
//! answers, live now, and carrying the digest of the body it came with
//! (ADR-0046); an answer saying nothing changed signed over its status,
//! the entity tag asked for and the URL it answers (ADR-0049).

use std::fmt;

use crate::digest::{CONTENT_DIGEST, digest_names};
use crate::keys::{Key, method};
use crate::message::{Component, Demands, Message, Presented, Refusal, presented};

/// The header naming what a body is.
pub const CONTENT_TYPE: &str = "content-type";

/// The header naming an answer's entity tag.
pub const ETAG: &str = "etag";

/// A peer's answer to a request.
#[derive(Debug, Clone, Copy)]
pub struct Answer<'a> {
    /// The request's method.
    pub method: &'a str,
    /// The request's full URL.
    pub url: &'a str,
    /// The answer's status code.
    pub status: u16,
    /// The answer's fields, in order.
    pub fields: &'a [(String, String)],
    /// The answer's body.
    pub body: &'a [u8],
}

/// What the reader holds an answer's signature to.
#[derive(Debug, Clone, Copy)]
pub struct Bounds {
    /// The most bytes a signature field or a digest may hold.
    pub fields: usize,
    /// Now, in seconds since the Unix epoch.
    pub now: i64,
    /// How many seconds its creation may lie ahead of now, or its expiry
    /// behind.
    pub skew: i64,
    /// The most seconds it may live.
    pub longest: i64,
}

/// Why an answer is not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unanswered {
    /// Its signature is missing, unreadable or does not meet the demands.
    Refused(Refusal),
    /// It carries no digest, or one of another body.
    Undigested,
    /// It is signed by a key the reader does not trust: its id.
    Stranger(String),
    /// It says nothing changed, its signature covering another entity tag
    /// than the one asked for.
    Retagged,
}

impl fmt::Display for Unanswered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(refusal) => refusal.fmt(f),
            Self::Undigested => write!(f, "the answer carries no digest of its body"),
            Self::Stranger(keyid) => {
                write!(f, "the answer is signed by {keyid}, a key not trusted")
            }
            Self::Retagged => write!(
                f,
                "the answer says nothing changed of another entity tag than the one asked for"
            ),
        }
    }
}

/// What every answer's signature must cover: its status, type and body's
/// digest, and the URL of the request it answers.
#[must_use]
pub fn demanded() -> [Component; 4] {
    [
        Component::Status,
        Component::Field(CONTENT_TYPE.to_owned()),
        Component::Field(CONTENT_DIGEST.to_owned()),
        Component::Request(Box::new(Component::TargetUri)),
    ]
}

/// What every answer saying nothing changed must cover: its status, its
/// entity tag, and the URL of the request it answers.
#[must_use]
pub fn demanded_unchanged() -> [Component; 3] {
    [
        Component::Status,
        Component::Field(ETAG.to_owned()),
        Component::Request(Box::new(Component::TargetUri)),
    ]
}

/// The key among `keys`, each of the instance on `domain`, that signed
/// `answer`, and its signature, whose bytes are still to be checked against
/// that key; checked against `bounds`, and against the digest of its body.
///
/// # Errors
///
/// Its signature is missing, unreadable, covers too little or is not live;
/// its digest is missing or of another body; or a key not among `keys`
/// signed it.
pub fn checked<'k>(
    answer: &Answer<'_>,
    domain: &str,
    keys: &'k [Key],
    bounds: Bounds,
) -> Result<(&'k Key, Presented), Unanswered> {
    let shown = met(answer, &demanded(), bounds)?;
    let digests: Vec<&str> = answer
        .fields
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case(CONTENT_DIGEST))
        .map(|(_, field)| field.as_str())
        .collect();
    if !digest_names(&digests.join(", "), answer.body, bounds.fields) {
        return Err(Unanswered::Undigested);
    }
    Ok((trusted(keys, domain, &shown)?, shown))
}

/// The key among `keys`, each of the instance on `domain`, that signed
/// `answer`, saying nothing changed of the entity tag `etag`, and its
/// signature, whose bytes are still to be checked against that key;
/// checked against `bounds`. Its body is not read.
///
/// # Errors
///
/// Its signature is missing, unreadable, covers too little or is not live;
/// it covers another entity tag than `etag`; or a key not among `keys`
/// signed it.
pub fn unchanged<'k>(
    answer: &Answer<'_>,
    domain: &str,
    keys: &'k [Key],
    bounds: Bounds,
    etag: &str,
) -> Result<(&'k Key, Presented), Unanswered> {
    let shown = met(answer, &demanded_unchanged(), bounds)?;
    if entity_tag(answer, &shown).as_deref() != Some(etag) {
        return Err(Unanswered::Retagged);
    }
    Ok((trusted(keys, domain, &shown)?, shown))
}

/// The entity tag `answer` carries, where its signature `shown` covers it.
#[must_use]
pub fn entity_tag(answer: &Answer<'_>, shown: &Presented) -> Option<String> {
    let tag = Component::Field(ETAG.to_owned());
    if !shown.covered.components.contains(&tag) {
        return None;
    }
    message(answer).value(&tag).ok()
}

/// `answer` as a message whose signature is read.
fn message<'a>(answer: &Answer<'a>) -> Message<'a> {
    Message {
        method: answer.method,
        url: answer.url,
        status: Some(answer.status),
        fields: answer.fields,
    }
}

/// The signature `answer` carries, meeting `components` within `bounds`.
fn met(
    answer: &Answer<'_>,
    components: &[Component],
    bounds: Bounds,
) -> Result<Presented, Unanswered> {
    let shown = presented(&message(answer), bounds.fields).map_err(Unanswered::Refused)?;
    shown
        .meets(Demands {
            components,
            single_use: false,
            now: bounds.now,
            skew: bounds.skew,
            longest: bounds.longest,
        })
        .map_err(Unanswered::Refused)?;
    Ok(shown)
}

/// The key among `keys`, each of the instance on `domain`, that `shown`
/// names.
fn trusted<'k>(keys: &'k [Key], domain: &str, shown: &Presented) -> Result<&'k Key, Unanswered> {
    keys.iter()
        .find(|key| method(domain, key) == shown.covered.keyid)
        .ok_or_else(|| Unanswered::Stranger(shown.covered.keyid.clone()))
}

#[cfg(test)]
mod tests;
