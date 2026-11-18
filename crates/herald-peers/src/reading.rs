//! A peer's feed as a reader reads it back: each page it lists, and each
//! envelope or tombstone on it, of the shape [`crate::feed`] writes
//! (ADR-0046).

use std::collections::BTreeMap;
use std::fmt;

use herald_core::Visibility;
use jiff::Timestamp;
use serde_json::{Map, Value};

use crate::feed::{ENVELOPE_MOST, EVENT, PAGE_MOST, TOMBSTONE};
use crate::jcs::{Uncanonical, canonical};

/// One page of a peer's feed, read back.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// Its envelopes, newest first, each under its sequence.
    pub items: Vec<(u64, Value)>,
    /// Whether older envelopes follow.
    pub more: bool,
}

/// Why a page, or an envelope on it, is not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unread {
    /// It is no JSON.
    Json,
    /// It is not of the shape herald writes: what is wrong.
    Shape(&'static str),
    /// It names another page than the one asked for: the page it names.
    Elsewhere(String),
    /// It lists more envelopes than any page may.
    Crowded(usize),
    /// Its sequences do not fall, newest first.
    Unordered,
    /// An envelope takes more bytes than any may.
    Oversized(usize),
    /// An envelope has no canonical form.
    Uncanonical(Uncanonical),
    /// An envelope's event is not public.
    Unpublic,
}

impl fmt::Display for Unread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json => write!(f, "the feed's page is no JSON"),
            Self::Shape(what) => write!(f, "the feed's page cannot be read: {what}"),
            Self::Elsewhere(id) => {
                write!(f, "the feed answered with the page {id}, not the one asked")
            }
            Self::Crowded(listed) => write!(
                f,
                "the feed's page lists {listed} envelopes, more than the {PAGE_MOST} any page may"
            ),
            Self::Unordered => write!(
                f,
                "the feed's page does not list its envelopes newest first"
            ),
            Self::Oversized(bytes) => write!(
                f,
                "an envelope takes {bytes} bytes, more than the {ENVELOPE_MOST} any may"
            ),
            Self::Uncanonical(why) => why.fmt(f),
            Self::Unpublic => write!(f, "an envelope carries an event that is not public"),
        }
    }
}

/// The page `body` answers for the URL `asked`: its envelopes, at most as
/// many as any page lists and each within the bytes any envelope takes,
/// newest first.
///
/// # Errors
///
/// It is no JSON, not of the shape herald writes, names another page, lists
/// too many envelopes or one too long, or not newest first.
pub fn page(body: &[u8], asked: &str) -> Result<Page, Unread> {
    let document: Value = serde_json::from_slice(body).map_err(|_| Unread::Json)?;
    let id = document
        .get("id")
        .and_then(Value::as_str)
        .ok_or(Unread::Shape("it names no id"))?;
    if id != asked {
        return Err(Unread::Elsewhere(id.to_owned()));
    }
    let listed = document
        .get("items")
        .and_then(Value::as_array)
        .ok_or(Unread::Shape("it lists no items"))?;
    if listed.len() > PAGE_MOST {
        return Err(Unread::Crowded(listed.len()));
    }
    let mut items: Vec<(u64, Value)> = Vec::with_capacity(listed.len());
    for item in listed {
        let sequence = item
            .get("sequence")
            .and_then(Value::as_u64)
            .ok_or(Unread::Shape("an item has no sequence"))?;
        if items.last().is_some_and(|(newer, _)| *newer <= sequence) {
            return Err(Unread::Unordered);
        }
        let envelope = item
            .get("envelope")
            .filter(|envelope| envelope.is_object())
            .ok_or(Unread::Shape("an item holds no envelope"))?;
        let bytes = canonical(envelope).map_err(Unread::Uncanonical)?.len();
        if bytes > ENVELOPE_MOST {
            return Err(Unread::Oversized(bytes));
        }
        items.push((sequence, envelope.clone()));
    }
    let more = document.get("next").is_some_and(Value::is_string);
    Ok(Page { items, more })
}

/// What an envelope carries, read back.
#[derive(Debug, Clone, PartialEq)]
pub enum Carried {
    /// An event, public.
    Event(Box<Enveloped>),
    /// A tombstone withdrawing the envelope of the event `identity`.
    Withdrawn {
        /// The withdrawn event's identity.
        identity: String,
        /// The instances it passed through, the origin first.
        path: Vec<String>,
    },
}

/// An event as its envelope carries it.
#[derive(Debug, Clone, PartialEq)]
pub struct Enveloped {
    /// Its identity, as its origin gave it.
    pub identity: String,
    /// Its kind's name.
    pub kind: String,
    /// Its project.
    pub project: String,
    /// Its title.
    pub title: String,
    /// Its body, in Markdown.
    pub body: String,
    /// Its links, by name.
    pub links: BTreeMap<String, String>,
    /// Its kind's attributes, as JSON.
    pub attributes: Map<String, Value>,
    /// When it happened.
    pub occurred: Timestamp,
    /// Its origin's forge, host and repository, where it names them.
    pub origin: Option<[String; 3]>,
    /// The instances it passed through, the origin first.
    pub path: Vec<String>,
}

/// What `envelope` carries: an event, public, or a tombstone.
///
/// # Errors
///
/// It is neither of the shape herald writes, or its event is not public.
pub fn carried(envelope: &Value) -> Result<Carried, Unread> {
    let text = |name: &str| envelope.get(name).and_then(Value::as_str);
    let path: Vec<String> = envelope
        .get("path")
        .and_then(Value::as_array)
        .filter(|path| !path.is_empty())
        .and_then(|path| {
            path.iter()
                .map(|hop| hop.as_str().map(str::to_owned))
                .collect()
        })
        .ok_or(Unread::Shape("it names no path of instances"))?;
    let identity = text("identity")
        .ok_or(Unread::Shape("it names no identity"))?
        .to_owned();
    match text("type") {
        Some(TOMBSTONE) => return Ok(Carried::Withdrawn { identity, path }),
        Some(EVENT) => {}
        _ => return Err(Unread::Shape("it is neither an event nor a tombstone")),
    }
    if text("visibility") != Some(Visibility::Public.name()) {
        return Err(Unread::Unpublic);
    }
    let field = |name: &str, wrong| text(name).map(str::to_owned).ok_or(Unread::Shape(wrong));
    let links = envelope
        .get("links")
        .and_then(Value::as_object)
        .and_then(|links| {
            links
                .iter()
                .map(|(name, url)| Some((name.clone(), url.as_str()?.to_owned())))
                .collect()
        })
        .ok_or(Unread::Shape("its links are not named URLs"))?;
    let occurred = text("occurred")
        .and_then(|occurred| occurred.parse().ok())
        .ok_or(Unread::Shape("it names no time it happened"))?;
    let origin = envelope.get("origin").map(origin_of).transpose()?;
    Ok(Carried::Event(Box::new(Enveloped {
        identity,
        kind: field("kind", "it names no kind")?,
        project: field("project", "it names no project")?,
        title: field("title", "it has no title")?,
        body: field("body", "it has no body")?,
        links,
        attributes: envelope
            .get("attributes")
            .and_then(Value::as_object)
            .cloned()
            .ok_or(Unread::Shape("its attributes are not an object"))?,
        occurred,
        origin,
        path,
    })))
}

/// The forge, host and repository `origin` names.
fn origin_of(origin: &Value) -> Result<[String; 3], Unread> {
    let named = |name: &str| origin.get(name).and_then(Value::as_str).map(str::to_owned);
    match (named("forge"), named("host"), named("repository")) {
        (Some(forge), Some(host), Some(repository)) => Ok([forge, host, repository]),
        _ => Err(Unread::Shape(
            "its origin names no forge, host and repository",
        )),
    }
}

#[cfg(test)]
mod tests;
