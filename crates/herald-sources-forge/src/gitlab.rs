use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::tz::TimeZone;
use serde::{Deserialize, Deserializer};

use crate::forge::{Api, Repository};
use herald_core::unreserved;
use herald_wire::SourceError;

use crate::wire::{ForgeRelease, Released, read};

pub(crate) const API: Api = Api {
    url: "https://gitlab.com/api/v4",
    page: "per_page",
    tagged: "/releases/",
    event: "GitLab's release event",
    repository: "GitLab's project",
    releases: "GitLab's releases",
    release: "GitLab's release",
    tags: "GitLab's tags",
    tag: "GitLab's tag",
    listing: "GitLab's projects",
    file: "GitLab's file",
    packages: "GitLab's packages",
    versions: "GitLab's package versions",
    listed_page: std::num::NonZeroUsize::new(100).unwrap(),
};

/// The suffix of the times in GitLab's webhooks, which are in UTC.
const UTC: &str = " UTC";

/// The action of GitLab's hook of a release created.
const CREATED: &str = "create";
/// The action of GitLab's hook of a release updated.
const UPDATED: &str = "update";
/// The action of GitLab's hook of a release deleted.
const DELETED: &str = "delete";

/// A release hook, as GitLab sends it.
#[derive(Deserialize)]
struct Hook {
    object_kind: String,
    action: String,
    tag: String,
    name: Option<String>,
    description: Option<String>,
    url: String,
    #[serde(default, deserialize_with = "when")]
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

impl From<Release> for ForgeRelease {
    fn from(release: Release) -> Self {
        Self {
            tag: release.tag_name,
            name: release.name,
            body: release.description,
            url: release.links.own,
            draft: release.upcoming,
            prerelease: None,
            published: release.released_at,
            released: Released::Published,
        }
    }
}

/// A time as GitLab writes it in a webhook, `2026-10-28 21:42:40 UTC`, or
/// as an RFC 3339 timestamp.
pub(crate) fn when<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Timestamp>, D::Error> {
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

/// The reference GitLab reads a file at where none is named: the default
/// branch.
pub(crate) const DEFAULT_REF: &str = "HEAD";

/// The GraphQL endpoint beside `api`, a GitLab REST API: `/api/graphql`
/// beside `/api/v4`, under any relative root the instance is served at.
pub(crate) fn graphql(api: &str) -> String {
    let api = api.trim_end_matches('/');
    match api.strip_suffix("/v4") {
        Some(base) => format!("{base}/graphql"),
        None => format!("{api}/graphql"),
    }
}

/// What a project's or a group's access token bot's username holds.
const TOKEN_BOT: &str = "_bot_";
/// What a service's bot's username ends with.
const BOT_ENDS: [&str; 2] = ["-bot", "[bot]"];

/// Whether `username` is a bot's: a project's or a group's access
/// token bot, or one ending in `-bot` or `[bot]`.
pub(crate) fn bot(username: &str) -> bool {
    username.contains(TOKEN_BOT) || BOT_ENDS.iter().any(|end| username.ends_with(end))
}

/// The web root of the instance whose REST API is `api`: `api` without
/// its `/api/v4`.
pub(crate) fn web(api: &str) -> String {
    let api = api.trim_end_matches('/');
    api.strip_suffix("/api/v4").unwrap_or(api).to_owned()
}

/// The base URL of a project's API, named by its full path.
pub(crate) fn project(api: &str, path: &str) -> String {
    format!("{api}/projects/{}", unreserved(path))
}

/// A project as a webhook payload names it in full.
#[derive(Deserialize)]
struct Named {
    path_with_namespace: String,
}

/// What a webhook payload is of.
#[derive(Deserialize)]
struct Located {
    project: Named,
}

/// The full path of the project a webhook `payload` is of.
pub(crate) fn path(what: &'static str, payload: &[u8]) -> Option<String> {
    read::<Located>(what, payload)
        .ok()
        .map(|located| located.project.path_with_namespace)
}

/// The project and release a `create` release hook names, an `update` one
/// names edited, or a `delete` one names withdrawn, or nothing for any
/// other action.
pub(crate) fn published(
    what: &'static str,
    payload: &[u8],
) -> Result<Option<(Repository, ForgeRelease)>, SourceError> {
    let hook: Hook = read(what, payload)?;
    if hook.object_kind != "release" {
        return Err(SourceError::Unreadable {
            what,
            reason: format!("it is a {} hook, not a release hook", hook.object_kind),
        });
    }
    let released = match hook.action.as_str() {
        CREATED => Released::Published,
        UPDATED => Released::Edited,
        DELETED => Released::Withdrawn,
        _ => return Ok(None),
    };
    let found = ForgeRelease {
        tag: hook.tag,
        name: hook.name,
        body: hook.description,
        url: hook.url,
        draft: false,
        prerelease: None,
        published: hook.released_at,
        released,
    };
    Ok(Some((hook.project, found)))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_graphql_endpoint_is_beside_the_rest_api_under_any_relative_root() {
        assert_eq!(graphql(API.url), "https://gitlab.com/api/graphql");
        assert_eq!(
            graphql("https://git.example/gitlab/api/v4/"),
            "https://git.example/gitlab/api/graphql"
        );
        assert_eq!(
            graphql("https://git.example/api"),
            "https://git.example/api/graphql"
        );
    }
}
