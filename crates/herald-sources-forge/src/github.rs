use jiff::Timestamp;
use serde::Deserialize;

use crate::forge::{Api, Repository};
use crate::wire::{ForgeRelease, Released, read};
use herald_wire::SourceError;

pub(crate) const API: Api = Api {
    url: herald_wire::github::API,
    page: "per_page",
    tagged: "/releases/tags/",
    event: "GitHub's release event",
    repository: "GitHub's repository",
    releases: "GitHub's releases",
    release: "GitHub's release",
    tags: "GitHub's tags",
    tag: "GitHub's tag",
    listing: "GitHub's repositories",
    file: "GitHub's file",
    packages: "GitHub's packages",
    versions: "GitHub's package versions",
    listed_page: std::num::NonZeroUsize::new(100).unwrap(),
};

/// The action of a release event of a release published.
const PUBLISHED: &str = "published";
/// The action of GitHub's release event of a release edited.
const EDITED: &str = "edited";
/// The action of Forgejo's release event of a release updated.
const UPDATED: &str = "updated";
/// The action of a release event of a release deleted.
const DELETED: &str = "deleted";

/// A release event, as GitHub and Forgejo send it.
#[derive(Deserialize)]
struct Payload {
    action: String,
    release: Release,
    repository: Repository,
}

/// A release, as GitHub and Forgejo describe it.
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    draft: bool,
    prerelease: bool,
    published_at: Option<Timestamp>,
}

impl From<Release> for ForgeRelease {
    fn from(release: Release) -> Self {
        Self {
            tag: release.tag_name,
            name: release.name,
            body: release.body,
            url: release.html_url,
            draft: release.draft,
            prerelease: Some(release.prerelease),
            published: release.published_at,
            released: Released::Published,
        }
    }
}

/// The repository and release a `published` release event names, one
/// GitHub's `edited` or Forgejo's `updated` names edited, or a `deleted`
/// one names withdrawn, or nothing for any other action.
pub(crate) fn published(
    what: &'static str,
    payload: &[u8],
) -> Result<Option<(Repository, ForgeRelease)>, SourceError> {
    let payload: Payload = read(what, payload)?;
    let released = match payload.action.as_str() {
        PUBLISHED => Released::Published,
        EDITED | UPDATED => Released::Edited,
        DELETED => Released::Withdrawn,
        _ => return Ok(None),
    };
    let release = ForgeRelease {
        released,
        ..payload.release.into()
    };
    Ok(Some((payload.repository, release)))
}

/// A repository as a webhook payload names it in full.
#[derive(Deserialize)]
struct Named {
    full_name: String,
}

/// What a webhook payload is of.
#[derive(Deserialize)]
struct Located {
    repository: Named,
}

/// The full path of the repository a webhook `payload` is of.
pub(crate) fn path(what: &'static str, payload: &[u8]) -> Option<String> {
    read::<Located>(what, payload)
        .ok()
        .map(|located| located.repository.full_name)
}

pub(crate) fn found(what: &'static str, body: &[u8]) -> Result<ForgeRelease, SourceError> {
    read::<Release>(what, body).map(ForgeRelease::from)
}

pub(crate) fn all_found(
    what: &'static str,
    limit: usize,
    body: &[u8],
) -> Result<Vec<ForgeRelease>, SourceError> {
    let releases: Vec<Release> = herald_wire::read(what, limit, body)?;
    Ok(releases.into_iter().map(ForgeRelease::from).collect())
}

/// The kind of account GitHub names a bot's or an app's.
const BOT: &str = "Bot";
/// What a bot's login ends with, on Forgejo too, which names no account
/// kind.
const BOT_SUFFIX: &str = "[bot]";

/// An account, as GitHub and Forgejo name one.
#[derive(Deserialize)]
pub(crate) struct Account {
    pub(crate) login: String,
    #[serde(rename = "type")]
    kind: Option<String>,
}

impl Account {
    /// Whether it is a bot's or an app's.
    pub(crate) fn bot(&self) -> bool {
        self.kind.as_deref() == Some(BOT) || self.login.ends_with(BOT_SUFFIX)
    }
}

/// A label, as GitHub and Forgejo name one.
#[derive(Deserialize)]
pub(crate) struct Label {
    pub(crate) name: String,
}
