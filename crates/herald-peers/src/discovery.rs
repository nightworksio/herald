//! The documents an instance is found by: its discovery document with its
//! key set, its WebFinger account, and NodeInfo 2.1 (ADR-0044).

use serde_json::{Value, json};

use crate::keys::{KeySet, WELL_KNOWN};

/// Where herald's API lives, under its base.
pub const API: &str = "/herald/v1";
/// The version of the API the discovery document describes.
pub const API_VERSION: u32 = 1;
/// The account WebFinger finds the instance by, before `@<domain>`.
pub const ACCOUNT: &str = "herald";
/// Where NodeInfo lists the documents it serves.
pub const NODEINFO_LINKS: &str = "/.well-known/nodeinfo";
/// Where herald serves its NodeInfo document.
pub const NODEINFO: &str = "/nodeinfo/2.1";
/// The NodeInfo schema herald answers by.
const NODEINFO_SCHEMA: &str = "http://nodeinfo.diaspora.software/ns/schema/2.1";
/// What a NodeInfo document is, as `Content-Type` names it.
pub const NODEINFO_JSON: &str =
    "application/json; profile=\"http://nodeinfo.diaspora.software/ns/schema/2.1#\"";
/// What a discovery document is, as `Content-Type` and WebFinger name it.
const JSON: &str = "application/json";
/// The protocol NodeInfo names where herald serves ActivityPub actors.
const ACTIVITYPUB: &str = "activitypub";
/// The feature the discovery document names where the instance serves
/// feeds.
const FEEDS: &str = "feeds";

/// The instance a document describes: where it is served, and what it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instance<'a> {
    /// Its domain.
    pub domain: &'a str,
    /// The path every endpoint is served under; empty at the root.
    pub base: &'a str,
    /// The software's name.
    pub software: &'a str,
    /// The software's version.
    pub version: &'a str,
    /// The software's repository.
    pub repository: &'a str,
}

impl Instance<'_> {
    /// The URL of its discovery document.
    #[must_use]
    pub fn discovery(&self) -> String {
        format!("https://{}{}{WELL_KNOWN}", self.domain, self.base)
    }

    /// Its discovery document: the API it serves, its software, its key
    /// set, and the feeds named `feeds` it serves.
    #[must_use]
    pub fn document(&self, keys: &KeySet, feeds: &[&str]) -> String {
        let features: Vec<&str> = (!feeds.is_empty()).then_some(FEEDS).into_iter().collect();
        let feeds: Vec<Value> = feeds
            .iter()
            .map(|name| json!({ "name": name, "url": self.feed(name) }))
            .collect();
        json!({
            "version": API_VERSION,
            "api": format!("https://{}{}{API}", self.domain, self.base),
            "software": { "name": self.software, "version": self.version },
            "keys": keys.document(self.domain),
            "features": features,
            "feeds": feeds,
        })
        .to_string()
    }

    /// Whether WebFinger's `resource` names the instance: its account or
    /// its domain's own URL.
    #[must_use]
    pub fn named_by(&self, resource: &str) -> bool {
        resource == format!("acct:{ACCOUNT}@{}", self.domain)
            || resource.trim_end_matches('/') == format!("https://{}", self.domain)
    }

    /// What WebFinger answers for the instance.
    #[must_use]
    pub fn finger(&self) -> String {
        json!({
            "subject": format!("acct:{ACCOUNT}@{}", self.domain),
            "aliases": [format!("https://{}/", self.domain)],
            "links": [{ "rel": "self", "type": JSON, "href": self.discovery() }],
        })
        .to_string()
    }

    /// Where its NodeInfo document is.
    #[must_use]
    pub fn nodeinfo_links(&self) -> String {
        json!({
            "links": [{
                "rel": NODEINFO_SCHEMA,
                "href": format!("https://{}{}{NODEINFO}", self.domain, self.base),
            }],
        })
        .to_string()
    }

    /// Its NodeInfo document, counting `actors` ActivityPub actors served
    /// on its domain, and naming the feeds named `feeds` it serves.
    #[must_use]
    pub fn nodeinfo(&self, actors: usize, feeds: &[&str]) -> String {
        let feeds: Vec<String> = feeds.iter().map(|name| self.feed(name)).collect();
        let protocols: Vec<&str> = (actors > 0).then_some(ACTIVITYPUB).into_iter().collect();
        json!({
            "version": "2.1",
            "software": {
                "name": self.software,
                "version": self.version,
                "repository": self.repository,
            },
            "protocols": protocols,
            "services": { "inbound": Value::Array(Vec::new()), "outbound": Value::Array(Vec::new()) },
            "openRegistrations": false,
            "usage": { "users": { "total": actors } },
            "metadata": { "discovery": self.discovery(), "feeds": feeds },
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests;
