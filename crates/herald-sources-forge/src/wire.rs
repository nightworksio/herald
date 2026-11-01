use std::collections::BTreeMap;

use herald_core::{Draft, Event, Kind, prerelease_version};
use herald_ports::Response;
use jiff::Timestamp;
use serde::de::DeserializeOwned;

use crate::error::ForgeError;
use crate::forge::Repository;

/// The most bytes a parser reads.
pub(crate) const LIMIT: usize = 4_194_304;

/// A release, as any forge describes it.
pub(crate) struct Found {
    pub(crate) tag: String,
    pub(crate) name: Option<String>,
    pub(crate) body: Option<String>,
    pub(crate) url: String,
    pub(crate) draft: bool,
    pub(crate) prerelease: bool,
    pub(crate) published: Option<Timestamp>,
}

pub(crate) fn read<T: DeserializeOwned>(what: &'static str, bytes: &[u8]) -> Result<T, ForgeError> {
    if bytes.len() > LIMIT {
        return Err(ForgeError::TooLarge { what, limit: LIMIT });
    }
    serde_json::from_slice(bytes).map_err(|error| ForgeError::Unreadable {
        what,
        reason: error.to_string(),
    })
}

pub(crate) fn answered(what: &'static str, response: &Response) -> Result<(), ForgeError> {
    if response.succeeded() {
        Ok(())
    } else {
        Err(ForgeError::Answered {
            what,
            status: response.status,
        })
    }
}

/// `text` as one segment of a URL's path.
pub(crate) fn segment(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', ':']).next().unwrap_or(rest)
}

/// The event a release of `repository` announces: a published release that
/// is not a draft, or nothing.
pub(crate) fn event(
    what: &'static str,
    repository: &Repository,
    found: Found,
) -> Result<Option<Event>, ForgeError> {
    if found.draft {
        return Ok(None);
    }
    let occurred = found.published.ok_or(ForgeError::Unreadable {
        what,
        reason: "a published release has no publication time".to_owned(),
    })?;
    let title = found
        .name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| found.tag.clone());
    let draft = Draft {
        identity: format!("{}/{}/{}", host(&found.url), repository.id, found.tag),
        kind: Kind::Release,
        project: repository.name.clone(),
        prerelease: found.prerelease || prerelease_version(&found.tag),
        version: Some(found.tag),
        title,
        body: found.body.unwrap_or_default(),
        links: BTreeMap::from([("release".to_owned(), found.url)]),
        occurred,
    };
    Event::normalise(draft)
        .map(Some)
        .map_err(|problem| ForgeError::Invalid { what, problem })
}
