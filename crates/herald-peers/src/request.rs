//! A peer's request checked before its body is read (ADR-0054): signed
//! once, by a nonce, over its method, URL, type and body's digest, and
//! live now; then, once its key's peer allows its size, carrying the
//! digest of the body it came with. Which peer's key signed it, and
//! whether the signature holds, the receiver decides.

use std::fmt;

use crate::answer::{Bounds, CONTENT_TYPE};
use crate::digest::{CONTENT_DIGEST, digest_names};
use crate::message::{Component, Demands, Message, Presented, Refusal, presented};

/// A peer's request.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// Its method.
    pub method: &'a str,
    /// Its full URL.
    pub url: &'a str,
    /// Its fields, in order.
    pub fields: &'a [(String, String)],
    /// Its body.
    pub body: &'a [u8],
}

/// Why a request is not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unrequested {
    /// Its signature is missing, unreadable or does not meet the demands.
    Refused(Refusal),
    /// It carries no digest, or one of another body.
    Undigested,
}

impl fmt::Display for Unrequested {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(refusal) => refusal.fmt(f),
            Self::Undigested => write!(f, "the request carries no digest of its body"),
        }
    }
}

/// What every request's signature must cover: its method, URL, type and
/// body's digest.
#[must_use]
pub fn demanded() -> [Component; 4] {
    [
        Component::Method,
        Component::TargetUri,
        Component::Field(CONTENT_TYPE.to_owned()),
        Component::Field(CONTENT_DIGEST.to_owned()),
    ]
}

/// A request whose signature meets the demands, its body not yet read.
#[derive(Debug, Clone)]
pub struct Signed<'a> {
    request: Request<'a>,
    fields: usize,
    nonce: String,
    shown: Presented,
}

impl Signed<'_> {
    /// The id of the key its signature names.
    #[must_use]
    pub fn keyid(&self) -> &str {
        &self.shown.covered.keyid
    }

    /// When its signature expires, in seconds since 1970.
    #[must_use]
    pub const fn expires(&self) -> i64 {
        self.shown.covered.expires
    }

    /// The nonce its signature carries.
    #[must_use]
    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    /// Its signature, whose key and bytes are still to be checked, once
    /// the request carries the digest of its body.
    ///
    /// # Errors
    ///
    /// Its digest is missing or of another body.
    pub fn digested(self) -> Result<Presented, Unrequested> {
        let digests: Vec<&str> = self
            .request
            .fields
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case(CONTENT_DIGEST))
            .map(|(_, field)| field.as_str())
            .collect();
        if !digest_names(&digests.join(", "), self.request.body, self.fields) {
            return Err(Unrequested::Undigested);
        }
        Ok(self.shown)
    }
}

/// The signature `request` carries, its body not yet read, once it meets
/// the demands within `bounds` and is single-use.
///
/// # Errors
///
/// Its signature is missing, unreadable, covers too little, carries no
/// nonce or is not live.
pub fn checked<'a>(request: &Request<'a>, bounds: Bounds) -> Result<Signed<'a>, Unrequested> {
    let message = Message {
        method: request.method,
        url: request.url,
        status: None,
        fields: request.fields,
    };
    let shown = presented(&message, bounds.fields).map_err(Unrequested::Refused)?;
    shown
        .meets(Demands {
            components: &demanded(),
            single_use: true,
            now: bounds.now,
            skew: bounds.skew,
            longest: bounds.longest,
        })
        .map_err(Unrequested::Refused)?;
    let nonce = shown.covered.nonce.clone();
    Ok(Signed {
        request: *request,
        fields: bounds.fields,
        nonce: nonce.ok_or(Unrequested::Refused(Refusal::Unnonced))?,
        shown,
    })
}

#[cfg(test)]
mod tests;
