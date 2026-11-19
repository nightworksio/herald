//! An instance's feeds as a static signed log any static host serves
//! (ADR-0060): each feed in pages of fixed buckets of sequences, and a
//! discovery document listing the digest of every page under the log's
//! sequence, with a detached signature beside it, so a reader takes no
//! page withheld, replayed or rolled back.

use std::fmt;

use herald_core::{Escaping, Feed, FeedItem};
use jiff::Timestamp;
use serde_json::{Map, Value, json};

use crate::digest::content_digest;
use crate::discovery::{API, Instance};
use crate::proof::{self, Shown, Unproven};

/// Where the older pages of a feed lie, under the API, before its name.
pub const PAGES: &str = "/pages/";
/// What the URL of a file's detached signature adds to the file's.
pub const SIGNATURE: &str = ".sig";
/// The type a signed statement names.
pub const SIGNED: &str = "herald:Signed";
/// The file telling GitHub Pages to serve every path as written, so it
/// serves `.well-known`.
pub const NOJEKYLL: &str = ".nojekyll";
/// How a statement writes when the log was written: in UTC, to the
/// second.
const PUBLISHED_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

/// When a static log was written, and under which sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Log {
    /// Its sequence, which no other writing of the log takes.
    pub sequence: u64,
    /// When it was written.
    pub published: Timestamp,
}

/// One file of a static log: its URL and its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// Its URL.
    pub url: String,
    /// Its body.
    pub body: String,
}

/// One feed as a static log's discovery document lists it: its name, the
/// sequence its newest envelope took, and its pages, newest first.
#[derive(Debug, Clone, Copy)]
pub struct Logged<'a> {
    /// Its name.
    pub name: &'a str,
    /// The sequence its newest envelope took.
    pub newest: u64,
    /// Its pages, newest first.
    pub pages: &'a [Written],
}

impl Instance<'_> {
    /// The URL of the page of the feed `name` listing the bucket `bucket`,
    /// where it is not the newest.
    #[must_use]
    pub fn page_of(&self, name: &str, bucket: u64) -> String {
        format!(
            "https://{}{}{API}{PAGES}{}/{bucket}",
            self.domain,
            self.base,
            Escaping::UNRESERVED.escape(name)
        )
    }

    /// The pages of `feed`, named `name`, newest first: one for each bucket
    /// of `size` sequences it lists any envelope of, the newest at the
    /// feed's URL, each naming the next; an empty feed one page listing
    /// nothing.
    #[must_use]
    pub fn pages(&self, name: &str, feed: &Feed, size: usize) -> Vec<Written> {
        let size = u64::try_from(size.max(1)).unwrap_or(u64::MAX);
        let mut buckets: Vec<(u64, Vec<&FeedItem>)> = Vec::new();
        for item in &feed.items {
            let bucket = item.sequence / size;
            match buckets.last_mut() {
                Some((at, items)) if *at == bucket => items.push(item),
                _ => buckets.push((bucket, vec![item])),
            }
        }
        let head = self.feed(name);
        let mut urls = vec![head.clone()];
        urls.extend(
            buckets
                .iter()
                .skip(1)
                .map(|(bucket, _)| self.page_of(name, *bucket)),
        );
        let listed = |items: &[&FeedItem]| -> Vec<Value> {
            items
                .iter()
                .map(|item| json!({ "sequence": item.sequence, "envelope": item.envelope }))
                .collect()
        };
        let mut pages = Vec::with_capacity(urls.len());
        for (at, url) in urls.iter().enumerate() {
            let items = buckets.get(at).map(|(_, items)| listed(items));
            let mut page = Map::from_iter([
                ("id".to_owned(), json!(url)),
                ("feed".to_owned(), json!(head)),
                ("items".to_owned(), Value::Array(items.unwrap_or_default())),
            ]);
            if let Some(next) = urls.get(at + 1) {
                page.insert("next".to_owned(), json!(next));
            }
            pages.push(Written {
                url: url.clone(),
                body: Value::Object(page).to_string(),
            });
        }
        pages
    }

    /// Its discovery document as a static log writes it: as
    /// [`Instance::document`] writes it, listing for each of `feeds` the
    /// sequence its newest envelope took and the URL and digest of each of
    /// its pages, under `log`.
    #[must_use]
    pub fn logged(&self, keys: &Value, feeds: &[Logged<'_>], log: Log) -> String {
        let listed = feeds
            .iter()
            .map(|feed| {
                let pages: Vec<Value> = feed
                    .pages
                    .iter()
                    .map(|page| {
                        json!({ "url": page.url, "digest": content_digest(page.body.as_bytes()) })
                    })
                    .collect();
                json!({
                    "name": feed.name,
                    "url": self.feed(feed.name),
                    "sequence": feed.newest,
                    "pages": pages,
                })
            })
            .collect();
        let mut document = self.described(keys, listed);
        document.insert("log".to_owned(), Value::Object(logged(log)));
        Value::Object(document).to_string()
    }
}

/// `log` as a document writes it.
fn logged(log: Log) -> Map<String, Value> {
    Map::from_iter([
        ("sequence".to_owned(), json!(log.sequence)),
        (
            "published".to_owned(),
            json!(log.published.strftime(PUBLISHED_FORMAT).to_string()),
        ),
    ])
}

/// The URL of the detached signature of the file at `url`.
#[must_use]
pub fn signature_of(url: &str) -> String {
    format!("{url}{SIGNATURE}")
}

/// The statement a detached signature proves, before its proof: that the
/// file at `url` holds `body`, as the log was written under `log`.
#[must_use]
pub fn statement(url: &str, body: &[u8], log: Log) -> Value {
    let mut statement = Map::from_iter([
        ("type".to_owned(), json!(SIGNED)),
        ("url".to_owned(), json!(url)),
        ("digest".to_owned(), json!(content_digest(body))),
    ]);
    statement.extend(logged(log));
    Value::Object(statement)
}

/// Why a static log's file or its signature is not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unsigned {
    /// It is no JSON.
    Json,
    /// It is not of the shape herald writes: what is wrong.
    Shape(&'static str),
    /// The signature names another file than the one read: the URL it
    /// names.
    Elsewhere(String),
    /// The signature names the digest of other bytes than the file's.
    Undigested,
    /// The signature's proof cannot be read.
    Unproven(Unproven),
    /// The log lists no feed at the URL read.
    Unlisted(String),
}

impl fmt::Display for Unsigned {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json => write!(f, "the static log's file is no JSON"),
            Self::Shape(what) => write!(f, "the static log cannot be read: {what}"),
            Self::Elsewhere(url) => write!(f, "the signature is of {url}, not the file read"),
            Self::Undigested => write!(
                f,
                "the signature names the digest of other bytes than the file's"
            ),
            Self::Unproven(why) => why.fmt(f),
            Self::Unlisted(feed) => write!(f, "the static log lists no feed at {feed}"),
        }
    }
}

/// A detached signature read back: the log it names, and the proof it
/// carries, whose signature is still to be checked against the key it
/// names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    /// The log it names.
    pub log: Log,
    /// Its proof.
    pub proof: Shown,
}

/// The statement `signature` makes of `body`, the file at `url`.
///
/// # Errors
///
/// It is no JSON, no statement of the shape herald writes, of another
/// file or of other bytes, or its proof cannot be read.
pub fn signed(signature: &[u8], url: &str, body: &[u8]) -> Result<Statement, Unsigned> {
    let statement: Value = serde_json::from_slice(signature).map_err(|_| Unsigned::Json)?;
    let text = |name: &str| statement.get(name).and_then(Value::as_str);
    if text("type") != Some(SIGNED) {
        return Err(Unsigned::Shape(
            "the signature is no herald:Signed statement",
        ));
    }
    let named = text("url").ok_or(Unsigned::Shape("the signature names no file"))?;
    if named != url {
        return Err(Unsigned::Elsewhere(named.to_owned()));
    }
    if text("digest") != Some(content_digest(body).as_str()) {
        return Err(Unsigned::Undigested);
    }
    let log = log_of(&statement)?;
    let proof = proof::shown(&statement).map_err(Unsigned::Unproven)?;
    Ok(Statement { log, proof })
}

/// The log `value` names.
fn log_of(value: &Value) -> Result<Log, Unsigned> {
    let sequence = value
        .get("sequence")
        .and_then(Value::as_u64)
        .ok_or(Unsigned::Shape("it names no sequence"))?;
    let published = value
        .get("published")
        .and_then(Value::as_str)
        .and_then(|published| published.parse().ok())
        .ok_or(Unsigned::Shape("it names no time it was written"))?;
    Ok(Log {
        sequence,
        published,
    })
}

/// One feed of a static log, read back from its discovery document: the
/// sequence its newest envelope took, and the URL and digest of each of
/// its pages, newest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    /// The sequence its newest envelope took.
    pub newest: u64,
    /// The URL and digest of each page, newest first.
    pub pages: Vec<(String, String)>,
}

/// The feed at `feed` as the static log's discovery document `body`
/// lists it, the document written under `log`.
///
/// # Errors
///
/// It is no JSON, names another log than `log`, lists no feed at `feed`,
/// or not of the shape herald writes.
pub fn listing(body: &[u8], log: Log, feed: &str) -> Result<Listing, Unsigned> {
    let document: Value = serde_json::from_slice(body).map_err(|_| Unsigned::Json)?;
    let named = log_of(document.get("log").unwrap_or(&Value::Null))?;
    if named != log {
        return Err(Unsigned::Shape(
            "its log is not the one its signature names",
        ));
    }
    let entry = document
        .get("feeds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|entry| entry.get("url").and_then(Value::as_str) == Some(feed))
        .ok_or_else(|| Unsigned::Unlisted(feed.to_owned()))?;
    let newest = entry
        .get("sequence")
        .and_then(Value::as_u64)
        .ok_or(Unsigned::Shape("a feed names no sequence"))?;
    let mut pages = Vec::new();
    for page in entry
        .get("pages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let text = |name: &str| page.get(name).and_then(Value::as_str).map(str::to_owned);
        let (Some(url), Some(digest)) = (text("url"), text("digest")) else {
            return Err(Unsigned::Shape("a page names no URL or no digest"));
        };
        pages.push((url, digest));
    }
    if pages.first().map(|(url, _)| url.as_str()) != Some(feed) {
        return Err(Unsigned::Shape("a feed's first page is not at its URL"));
    }
    Ok(Listing { newest, pages })
}

#[cfg(test)]
mod tests;
