use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::tz::TimeZone;
use serde::{Deserialize, Deserializer};

use crate::forge::{Api, Repository};
use herald_wire::SourceError;
use herald_wire::segment;

use crate::wire::{Found, read};

pub(crate) const API: Api = Api {
    url: "https://gitlab.com/api/v4",
    page: "per_page",
    tagged: "/releases/",
    event: "GitLab's release event",
    repository: "GitLab's project",
    releases: "GitLab's releases",
    release: "GitLab's release",
};

/// The suffix of the times in GitLab's webhooks, which are in UTC.
const UTC: &str = " UTC";

/// A release hook, as GitLab sends it.
#[derive(Deserialize)]
struct Hook {
    object_kind: String,
    action: String,
    tag: String,
    name: Option<String>,
    description: Option<String>,
    url: String,
    #[serde(deserialize_with = "when")]
    released_at: Option<Timestamp>,
    project: Repository,
}

/// A release, as GitLab's API describes it.
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    name: Option<String>,
    description: Option<String>,
    released_at: Option<Timestamp>,
    #[serde(default, rename = "upcoming_release")]
    upcoming: bool,
    #[serde(rename = "_links")]
    links: Links,
}

#[derive(Deserialize)]
struct Links {
    #[serde(rename = "self")]
    own: String,
}

impl From<Release> for Found {
    fn from(release: Release) -> Self {
        Self {
            tag: release.tag_name,
            name: release.name,
            body: release.description,
            url: release.links.own,
            draft: release.upcoming,
            prerelease: false,
            published: release.released_at,
        }
    }
}

/// A time as GitLab writes it in a webhook, `2026-10-28 21:42:40 UTC`, or
/// as an RFC 3339 timestamp.
fn when<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Timestamp>, D::Error> {
    let Some(text) = Option::<String>::deserialize(deserializer)? else {
        return Ok(None);
    };
    let parsed = match text.strip_suffix(UTC) {
        Some(civil) => civil
            .parse::<DateTime>()
            .and_then(|civil| civil.to_zoned(TimeZone::UTC))
            .map(|zoned| zoned.timestamp()),
        None => text.parse::<Timestamp>(),
    };
    parsed.map(Some).map_err(serde::de::Error::custom)
}

/// The base URL of a project's API, named by its full path.
pub(crate) fn project(api: &str, path: &str) -> String {
    format!("{api}/projects/{}", segment(path))
}

/// The project and release a `create` release hook names, or nothing for
/// any other action.
pub(crate) fn published(
    what: &'static str,
    payload: &[u8],
) -> Result<Option<(Repository, Found)>, SourceError> {
    let hook: Hook = read(what, payload)?;
    if hook.object_kind != "release" {
        return Err(SourceError::Unreadable {
            what,
            reason: format!("it is a {} hook, not a release hook", hook.object_kind),
        });
    }
    if hook.action != "create" {
        return Ok(None);
    }
    let found = Found {
        tag: hook.tag,
        name: hook.name,
        body: hook.description,
        url: hook.url,
        draft: false,
        prerelease: false,
        published: hook.released_at,
    };
    Ok(Some((hook.project, found)))
}

pub(crate) fn found(what: &'static str, body: &[u8]) -> Result<Found, SourceError> {
    read::<Release>(what, body).map(Found::from)
}

pub(crate) fn all_found(what: &'static str, body: &[u8]) -> Result<Vec<Found>, SourceError> {
    let releases: Vec<Release> = read(what, body)?;
    Ok(releases.into_iter().map(Found::from).collect())
}
