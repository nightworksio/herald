use herald_ports::{Request, Text};
use jiff::Timestamp;
use serde::Deserialize;

use crate::forge::{Api, Repository};
use crate::wire::{Found, read};
use herald_wire::SourceError;

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
) -> Result<Option<(Repository, Found)>, SourceError> {
    let payload: Payload = read(what, payload)?;
    Ok((payload.action == "published").then(|| (payload.repository, payload.release.into())))
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

pub(crate) fn found(what: &'static str, body: &[u8]) -> Result<Found, SourceError> {
    read::<Release>(what, body).map(Found::from)
}

pub(crate) fn all_found(
    what: &'static str,
    limit: usize,
    body: &[u8],
) -> Result<Vec<Found>, SourceError> {
    let releases: Vec<Release> = herald_wire::read(what, limit, body)?;
    Ok(releases.into_iter().map(Found::from).collect())
}
