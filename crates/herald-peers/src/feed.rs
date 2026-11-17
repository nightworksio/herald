//! An instance's feeds: each event it publishes as an envelope proved by
//! its key, listed newest first in pages a reader walks with `?before=`.

use std::fmt;

use herald_core::{Escaping, Event, KindSpec, RELEASE_LINK, Size, Visibility};
use jiff::Timestamp;
use serde_json::{Map, Value, json};

use crate::discovery::{API, Instance, address};
use crate::jcs::{Uncanonical, canonical};
use crate::proof::PROOF;

/// Where the feeds are served, under the API, before a feed's name.
pub const FEEDS: &str = "/feeds/";
/// The query naming the sequence a page lists the envelopes before.
pub const BEFORE: &str = "before";
/// The type an envelope carrying an event names.
pub const EVENT: &str = "herald:Event";
/// The type a tombstone names: an envelope withdrawn.
pub const TOMBSTONE: &str = "herald:Tombstone";
/// The member naming what an envelope or tombstone is.
const TYPE: &str = "type";
/// How a tombstone writes when its event was withdrawn: in UTC, to the
/// second.
const DELETED_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";
/// The bytes left in an envelope for the proof it carries.
pub const PROOF_ROOM: usize = 512;
/// The envelopes a page lists where its feed says nothing.
pub const PAGE: usize = 20;
/// The most envelopes a page lists.
pub const PAGE_MOST: usize = 4 * PAGE;
/// The bytes an envelope takes at most where its feed says nothing.
pub const ENVELOPE: usize = 16 * Size::KIB;
/// The most bytes an envelope takes, its proof counted.
pub const ENVELOPE_MOST: usize = 4 * ENVELOPE;

/// Why an event has no envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unenveloped {
    /// The event has no canonical form.
    Uncanonical(Uncanonical),
    /// The envelope takes more than the bytes it may hold even with its
    /// body cut: how many it would take, and how many it may.
    Oversized {
        /// The bytes it would take.
        bytes: usize,
        /// The most it may take.
        most: usize,
    },
}

impl fmt::Display for Unenveloped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uncanonical(why) => why.fmt(f),
            Self::Oversized { bytes, most } => write!(
                f,
                "the event's envelope takes {bytes} bytes with its body cut, more than the {most} it may"
            ),
        }
    }
}

impl Instance<'_> {
    /// The URL of the feed `name`, its name escaped as one segment.
    #[must_use]
    pub fn feed(&self, name: &str) -> String {
        feed_of(&format!("https://{}{}", self.domain, self.base), name)
    }

    /// `event`'s envelope, before its proof: what of the event is public
    /// (its identity, kind, project, title, body, links, the attributes its
    /// kind `spec` declares, when it happened, and its forge, host and
    /// repository), that it is public, its type, the instances it passed
    /// through, starting with this one, and its evidence, within `most`
    /// bytes once proved, its body cut where it would take more.
    ///
    /// # Errors
    ///
    /// The event has no canonical form, or takes more than `most` bytes
    /// with no body.
    pub fn envelope(
        &self,
        event: &Event,
        spec: Option<&KindSpec>,
        most: usize,
    ) -> Result<Value, Unenveloped> {
        let mut envelope = public(event, spec);
        envelope.insert(TYPE.to_owned(), Value::String(EVENT.to_owned()));
        envelope.insert("path".to_owned(), json!([self.discovery()]));
        if let Some(release) = event.link(RELEASE_LINK) {
            envelope.insert("evidence".to_owned(), json!({ RELEASE_LINK: release }));
        }
        let room = most.saturating_sub(PROOF_ROOM);
        let whole = measured(&envelope)?;
        if whole <= room {
            return Ok(Value::Object(envelope));
        }
        envelope.insert("cut".to_owned(), Value::Bool(true));
        let excess = measured(&envelope)?.saturating_sub(room);
        let body = event.body();
        let end = body.floor_char_boundary(body.len().saturating_sub(excess));
        envelope.insert("body".to_owned(), Value::String(body[..end].to_owned()));
        let bytes = measured(&envelope)?;
        if bytes > room {
            return Err(Unenveloped::Oversized { bytes, most: room });
        }
        Ok(Value::Object(envelope))
    }
}

impl Instance<'_> {
    /// The tombstone withdrawing the envelope of the event `identity` at
    /// `deleted`, before its proof.
    #[must_use]
    pub fn tombstone(&self, identity: &str, deleted: Timestamp) -> Value {
        json!({
            TYPE: TOMBSTONE,
            "identity": identity,
            "deleted": deleted.strftime(DELETED_FORMAT).to_string(),
            "path": [self.discovery()],
        })
    }
}

/// The URL of the feed `name` of the instance served at `url`, its name
/// escaped as one segment.
#[must_use]
pub fn feed_of(url: &str, name: &str) -> String {
    format!("{url}{API}{FEEDS}{}", Escaping::UNRESERVED.escape(name))
}

/// A feed's URL read back: the URL itself, where its instance is served,
/// and the instance's domain.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FeedAt {
    /// The feed's URL, with no trailing `/`.
    pub feed: String,
    /// Where its instance is served.
    pub instance: String,
    /// Its instance's domain.
    pub domain: String,
}

/// The feed `url` names, as [`crate::discovery::address`] reads it;
/// `None` where `url` is no feed's URL.
#[must_use]
pub fn feed_at(url: &str) -> Option<FeedAt> {
    let (feed, domain) = address(url)?;
    let (instance, name) = feed.rsplit_once(&format!("{API}{FEEDS}"))?;
    (!name.is_empty() && !name.contains('/')).then(|| FeedAt {
        instance: instance.to_owned(),
        feed: feed.clone(),
        domain,
    })
}

/// An envelope or tombstone, as JSON.
pub type Envelope = Value;

/// Whether `listed`, the proof it carries left out, is `envelope`.
#[must_use]
pub fn shows(listed: &Value, envelope: &Value) -> bool {
    let mut bare = listed.clone();
    if let Some(fields) = bare.as_object_mut() {
        fields.remove(PROOF);
    }
    bare == *envelope
}

/// The identity of the event `envelope` carries; empty where it names
/// none.
#[must_use]
pub fn identity(envelope: &Value) -> &str {
    envelope
        .get("identity")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// Whether `envelope` is a tombstone.
#[must_use]
pub fn withdrawn(envelope: &Value) -> bool {
    envelope.get(TYPE).and_then(Value::as_str) == Some(TOMBSTONE)
}

/// What of `event` is public: never what its source keeps to itself, such
/// as its origin's id, topics, custom properties or the source it reached
/// herald through, nor an attribute its kind `spec` does not declare.
fn public(event: &Event, spec: Option<&KindSpec>) -> Map<String, Value> {
    let attributes: Map<String, Value> = spec
        .into_iter()
        .flat_map(|spec| spec.attributes.keys())
        .filter_map(|name| Some((name.clone(), json!(event.attribute(name)?))))
        .collect();
    let mut fields = Map::new();
    fields.insert("identity".to_owned(), json!(event.identity().as_str()));
    fields.insert("kind".to_owned(), json!(event.kind().as_str()));
    fields.insert("project".to_owned(), json!(event.project()));
    fields.insert("title".to_owned(), json!(event.title()));
    fields.insert("body".to_owned(), json!(event.body()));
    fields.insert("links".to_owned(), json!(event.links()));
    fields.insert("attributes".to_owned(), Value::Object(attributes));
    fields.insert("occurred".to_owned(), json!(event.occurred()));
    fields.insert("visibility".to_owned(), json!(Visibility::Public.name()));
    if let Some(origin) = event.origin() {
        fields.insert(
            "origin".to_owned(),
            json!({ "forge": origin.forge, "host": origin.host, "repository": origin.repository }),
        );
    }
    fields
}

/// The bytes `envelope` takes in its canonical form.
fn measured(envelope: &Map<String, Value>) -> Result<usize, Unenveloped> {
    canonical(&Value::Object(envelope.clone()))
        .map(|text| text.len())
        .map_err(Unenveloped::Uncanonical)
}

/// One envelope a feed lists, under its sequence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Listed<'a> {
    /// Its sequence in the feed.
    pub sequence: u64,
    /// The envelope, proved.
    pub envelope: &'a Value,
}

/// One page of a feed.
#[derive(Debug, Clone, PartialEq)]
pub struct Page<'a> {
    /// The feed's URL.
    pub feed: &'a str,
    /// The sequence it lists the envelopes before, where it is not the
    /// first.
    pub before: Option<u64>,
    /// Its envelopes, newest first.
    pub listed: Vec<Listed<'a>>,
    /// Whether older envelopes follow.
    pub more: bool,
}

impl Page<'_> {
    /// Its URL.
    #[must_use]
    pub fn id(&self) -> String {
        self.before.map_or_else(
            || self.feed.to_owned(),
            |before| format!("{}?{BEFORE}={before}", self.feed),
        )
    }

    /// The page as JSON: its URL, its feed, its envelopes, and the page
    /// after it where older envelopes follow.
    #[must_use]
    pub fn document(&self) -> String {
        let items: Vec<Value> = self
            .listed
            .iter()
            .map(|listed| json!({ "sequence": listed.sequence, "envelope": listed.envelope }))
            .collect();
        let mut page = Map::new();
        page.insert("id".to_owned(), Value::String(self.id()));
        page.insert("feed".to_owned(), Value::String(self.feed.to_owned()));
        page.insert("items".to_owned(), Value::Array(items));
        if let Some(oldest) = self.listed.last().filter(|_| self.more) {
            let next = format!("{}?{BEFORE}={}", self.feed, oldest.sequence);
            page.insert("next".to_owned(), Value::String(next));
        }
        Value::Object(page).to_string()
    }
}

/// The strong entity tag of a page listing `page` envelopes before
/// `before` of a feed whose newest sequence is `newest`: any change to the
/// feed raises its newest sequence.
#[must_use]
pub fn tag(newest: u64, before: Option<u64>, page: usize) -> String {
    format!("\"{newest}.{}.{page}\"", before.unwrap_or_default())
}

#[cfg(test)]
mod tests;
