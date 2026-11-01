use herald_ports::{Request, Text};
use jiff::Timestamp;
use serde::Deserialize;

use crate::error::ForgeError;
use crate::forge::{Api, Repository};
use crate::wire::{Found, read};

/// The API version herald was written against.
const API_VERSION: &str = "2022-11-28";

pub(crate) const API: Api = Api {
    url: "https://api.github.com",
    page: "per_page",
    tagged: "/releases/tags/",
    event: "GitHub's release event",
    repository: "GitHub's repository",
    releases: "GitHub's releases",
    release: "GitHub's release",
};

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

impl From<Release> for Found {
    fn from(release: Release) -> Self {
        Self {
            tag: release.tag_name,
            name: release.name,
            body: release.body,
            url: release.html_url,
            draft: release.draft,
            prerelease: release.prerelease,
            published: release.published_at,
        }
    }
}

/// GitHub's request headers.
pub(crate) fn headed(request: Request) -> Request {
    request
        .header("Accept", Text::plain("application/vnd.github+json"))
        .header("X-GitHub-Api-Version", Text::plain(API_VERSION))
}

/// The repository and release a `published` release event names, or
/// nothing for any other action.
pub(crate) fn published(
    what: &'static str,
    payload: &[u8],
) -> Result<Option<(Repository, Found)>, ForgeError> {
    let payload: Payload = read(what, payload)?;
    Ok((payload.action == "published").then(|| (payload.repository, payload.release.into())))
}

pub(crate) fn found(what: &'static str, body: &[u8]) -> Result<Found, ForgeError> {
    read::<Release>(what, body).map(Found::from)
}

pub(crate) fn all_found(what: &'static str, body: &[u8]) -> Result<Vec<Found>, ForgeError> {
    let releases: Vec<Release> = read(what, body)?;
    Ok(releases.into_iter().map(Found::from).collect())
}
