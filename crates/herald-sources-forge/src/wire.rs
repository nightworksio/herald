use std::collections::BTreeMap;

use herald_core::{
    Address, Attribute, Draft, EDITED, Event, Identity, Kind, PrereleaseRule, RELEASE_LINK, SIGNAL,
    Signal, WITHDRAWN, versioned,
};
use jiff::Timestamp;

use herald_wire::SourceError;
use serde::de::DeserializeOwned;

use crate::forge::Repository;

/// The most bytes a parser reads.
pub(crate) const LIMIT: usize = herald_wire::PARSE_LIMIT;

/// A release, as any forge describes it.
pub(crate) struct ForgeRelease {
    pub(crate) tag: String,
    pub(crate) name: Option<String>,
    pub(crate) body: Option<String>,
    pub(crate) url: String,
    pub(crate) draft: bool,
    pub(crate) prerelease: Option<bool>,
    pub(crate) published: Option<Timestamp>,
    pub(crate) released: Released,
}

/// What the forge did to a release a webhook names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Released {
    /// Published it.
    Published,
    /// Edited it.
    Edited,
    /// Deleted it (ADR-0048).
    Withdrawn,
}

impl Released {
    /// The attribute an event of a release it did to is marked with, where
    /// it is marked.
    const fn marked(self) -> Option<&'static str> {
        match self {
            Self::Published => None,
            Self::Edited => Some(EDITED),
            Self::Withdrawn => Some(WITHDRAWN),
        }
    }
}

/// A tag, read in full: its name, its message where it is annotated, and
/// its time, the annotated tag's date or else its commit's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForgeTag {
    pub(crate) name: String,
    pub(crate) message: Option<String>,
    pub(crate) date: Option<Timestamp>,
}

/// The identity every signal of the tag `tag` of the repository `id` on
/// `host` shares.
pub(crate) fn identity(host: &str, id: u64, tag: &str) -> Identity {
    Identity::new(format!("{host}/{id}/{tag}"))
}

/// `bytes` read as JSON, within the bytes a forge parser reads.
pub(crate) fn read<T: DeserializeOwned>(
    what: &'static str,
    bytes: &[u8],
) -> Result<T, SourceError> {
    herald_wire::read(what, LIMIT, bytes)
}

/// What one event of a repository is, whatever signal it is of.
struct Signalled {
    signal: Signal,
    tag: String,
    title: Option<String>,
    body: Option<String>,
    url: String,
    flag: Option<bool>,
    occurred: Timestamp,
    released: Released,
}

/// The event `signalled` of `repository` makes, its pre-release told by
/// `rule`, sharing its identity with every other signal of its tag.
fn drafted(
    what: &'static str,
    repository: &Repository,
    signalled: Signalled,
    rule: PrereleaseRule,
) -> Result<Event, SourceError> {
    let forge = Address::parse(&signalled.url).ok_or_else(|| SourceError::Unreadable {
        what,
        reason: "the page it links names no host".to_owned(),
    })?;
    let title = signalled
        .title
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| signalled.tag.clone());
    let mut attributes = versioned(
        signalled.tag.clone(),
        rule.marks(signalled.flag, &signalled.tag),
    );
    attributes.insert(
        SIGNAL.to_owned(),
        Attribute::Text(signalled.signal.name().to_owned()),
    );
    if let Some(marked) = signalled.released.marked() {
        attributes.insert(marked.to_owned(), Attribute::Flag(true));
    }
    let draft = Draft {
        identity: identity(forge.host(), repository.id, &signalled.tag)
            .as_str()
            .to_owned(),
        kind: Kind::RELEASE,
        project: repository.name.clone(),
        attributes,
        title,
        body: signalled.body.unwrap_or_default(),
        links: BTreeMap::from([(RELEASE_LINK.to_owned(), signalled.url)]),
        occurred: signalled.occurred,
    };
    Event::normalise(draft).map_err(|problem| SourceError::Invalid { what, problem })
}

/// The event a release of `repository` announces, its pre-release told by
/// `rule`: a published release that is not a draft, or nothing.
pub(crate) fn event(
    what: &'static str,
    repository: &Repository,
    found: ForgeRelease,
    rule: PrereleaseRule,
) -> Result<Option<Event>, SourceError> {
    if found.draft {
        return Ok(None);
    }
    let occurred = found.published.ok_or(SourceError::Unreadable {
        what,
        reason: "a published release has no publication time".to_owned(),
    })?;
    let signalled = Signalled {
        signal: Signal::Release,
        tag: found.tag,
        title: found.name,
        body: found.body,
        url: found.url,
        flag: found.prerelease,
        occurred,
        released: found.released,
    };
    drafted(what, repository, signalled, rule).map(Some)
}

/// The event the tag `found` of `repository` announces, at `url`, its
/// pre-release told by `rule`.
pub(crate) fn tag_event(
    what: &'static str,
    repository: &Repository,
    (found, url): (ForgeTag, String),
    rule: PrereleaseRule,
) -> Result<Event, SourceError> {
    let occurred = found.date.ok_or(SourceError::Unreadable {
        what,
        reason: format!("the tag {} has no time", found.name),
    })?;
    let signalled = Signalled {
        signal: Signal::Tag,
        tag: found.name,
        title: None,
        body: found.message,
        url,
        flag: None,
        occurred,
        released: Released::Published,
    };
    drafted(what, repository, signalled, rule)
}
