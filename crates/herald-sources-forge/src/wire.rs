use std::collections::BTreeMap;

use herald_core::{Draft, Event, Kind, prerelease_version};
use jiff::Timestamp;

use herald_wire::{SourceError, host};
use serde::de::DeserializeOwned;

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

/// `bytes` read as JSON, within the bytes a forge parser reads.
pub(crate) fn read<T: DeserializeOwned>(
    what: &'static str,
    bytes: &[u8],
) -> Result<T, SourceError> {
    herald_wire::read(what, LIMIT, bytes)
}

/// The event a release of `repository` announces: a published release that
/// is not a draft, or nothing.
pub(crate) fn event(
    what: &'static str,
    repository: &Repository,
    found: Found,
) -> Result<Option<Event>, SourceError> {
    if found.draft {
        return Ok(None);
    }
    let occurred = found.published.ok_or(SourceError::Unreadable {
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
        .map_err(|problem| SourceError::Invalid { what, problem })
}
