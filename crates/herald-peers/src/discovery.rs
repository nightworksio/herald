//! The documents an instance is found by: its discovery document with its
//! key set, its WebFinger account, and NodeInfo 2.1 (ADR-0044).

use std::net::Ipv4Addr;

use herald_core::{Address, domain};
use serde_json::{Value, json};

use crate::keys::{KeySet, Rotation, Unkeyed, Unrotated, WELL_KNOWN, rotation};

/// Where herald's API lives, under its base.
pub const API: &str = "/herald/v1";
/// Where a herald source's inbox lives under the API, before its name.
pub const INBOX: &str = "/inbox/";
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

/// The scheme an instance is reached by.
pub const SCHEME: &str = "https";

/// `url` as an instance's address, where it is one: `https://`, a domain
/// and a path, with no port, user, query or fragment. It answers the URL
/// with no trailing `/`, and the domain.
#[must_use]
pub fn address(url: &str) -> Option<(String, String)> {
    let address = Address::parse(url).filter(|address| {
        address.scheme() == SCHEME
            && domain(address.host())
            && address.host().parse::<Ipv4Addr>().is_err()
            && address.authority() == address.host()
            && !address.names_a_user()
            && !address.target().contains('?')
            && !url.contains('#')
    })?;
    let host = address.host().to_owned();
    let path = address.target().trim_end_matches('/');
    Some((format!("{SCHEME}://{host}{path}"), host))
}

/// The key set the discovery document `body` of the instance on `domain`
/// lists.
///
/// # Errors
///
/// It is no JSON, or lists no key set of the shape [`KeySet::document`]
/// writes, or a key that is not the instance's own.
pub fn key_set(body: &[u8], domain: &str) -> Result<KeySet, Unkeyed> {
    KeySet::read(&keys(body)?, domain)
}

/// The key set the discovery document `body` lists, as written, with any
/// proof it carries.
///
/// # Errors
///
/// It is no JSON.
pub fn keys(body: &[u8]) -> Result<Value, Unkeyed> {
    let mut document: Value =
        serde_json::from_slice(body).map_err(|_| Unkeyed::Shape("it is no JSON"))?;
    Ok(document
        .get_mut("keys")
        .map(Value::take)
        .unwrap_or_default())
}

/// The key set the discovery document `body` of the instance on `domain`
/// lists, as a rotation statement checked against the `held` set: proved
/// by a key of `held`, with a sequence past its own (ADR-0049). Its
/// proof's signature is still to be checked against [`Rotation::by`].
///
/// # Errors
///
/// It is no JSON, its set or proof cannot be read, a key `held` does not
/// hold proved it, or its sequence is not past `held`'s.
pub fn handed_over(body: &[u8], held: &KeySet, domain: &str) -> Result<Rotation, Unrotated> {
    rotation(&keys(body).map_err(Unrotated::Unkeyed)?, held, domain)
}

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
    /// set `keys` as [`KeySet::document`] writes it, proved or not, and the
    /// feeds named `feeds` it serves.
    #[must_use]
    pub fn document(&self, keys: &Value, feeds: &[&str]) -> String {
        let features: Vec<&str> = (!feeds.is_empty()).then_some(FEEDS).into_iter().collect();
        let feeds: Vec<Value> = feeds
            .iter()
            .map(|name| json!({ "name": name, "url": self.feed(name) }))
            .collect();
        json!({
            "version": API_VERSION,
            "api": format!("https://{}{}{API}", self.domain, self.base),
            "software": { "name": self.software, "version": self.version },
            "keys": keys,
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
