use std::collections::BTreeMap;

use herald_core::{Cursor, Draft, Event, Kind, prerelease_version};
use herald_ports::{Method, Request, Response, Secret, Text, USER_AGENT};
use jiff::Timestamp;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::error::ForgeError;
use crate::options::SourceOptions;

/// GitHub's own API.
const API: &str = "https://api.github.com";
/// The API version herald was written against.
const API_VERSION: &str = "2022-11-28";
/// The most bytes a parser reads.
const LIMIT: usize = 4_194_304;
/// The releases asked for in one poll.
const PAGE: u8 = 30;
/// What GitHub answers to a conditional request when nothing changed.
const NOT_MODIFIED: u16 = 304;
const EVENT: &str = "GitHub's release event";
const RELEASES: &str = "GitHub's releases";
const REPOSITORY: &str = "GitHub's repository";
const RELEASE: &str = "GitHub's release";

#[derive(Deserialize)]
struct Payload {
    action: String,
    release: Release,
    repository: Repository,
}

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

/// A repository, as GitHub names it: its numeric id outlives a rename.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Repository {
    /// The numeric id.
    pub id: u64,
    /// The name, without its owner.
    pub name: String,
}

/// The published releases a poll found, the newest first, and the
/// validators to send next time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Releases {
    /// The releases, as events.
    pub events: Vec<Event>,
    /// The `ETag` GitHub returned.
    pub etag: Option<String>,
    /// The `Last-Modified` GitHub returned.
    pub last_modified: Option<String>,
}

/// One GitHub repository's releases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHub {
    api: String,
    repository: String,
}

fn read<T: DeserializeOwned>(what: &'static str, bytes: &[u8]) -> Result<T, ForgeError> {
    if bytes.len() > LIMIT {
        return Err(ForgeError::TooLarge { what, limit: LIMIT });
    }
    serde_json::from_slice(bytes).map_err(|error| ForgeError::Unreadable {
        what,
        reason: error.to_string(),
    })
}

fn answered(what: &'static str, response: &Response) -> Result<(), ForgeError> {
    if response.succeeded() {
        Ok(())
    } else {
        Err(ForgeError::Answered {
            what,
            status: response.status,
        })
    }
}

fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', ':']).next().unwrap_or(rest)
}

fn release(
    what: &'static str,
    repository: &Repository,
    release: Release,
) -> Result<Option<Event>, ForgeError> {
    if release.draft {
        return Ok(None);
    }
    let occurred = release.published_at.ok_or(ForgeError::Unreadable {
        what,
        reason: "a published release has no published_at".to_owned(),
    })?;
    let title = release
        .name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| release.tag_name.clone());
    let draft = Draft {
        identity: format!(
            "{}/{}/{}",
            host(&release.html_url),
            repository.id,
            release.tag_name
        ),
        kind: Kind::Release,
        project: repository.name.clone(),
        prerelease: release.prerelease || prerelease_version(&release.tag_name),
        version: Some(release.tag_name),
        title,
        body: release.body.unwrap_or_default(),
        links: BTreeMap::from([("release".to_owned(), release.html_url)]),
        occurred,
    };
    Event::normalise(draft)
        .map(Some)
        .map_err(|problem| ForgeError::Invalid { what, problem })
}

impl GitHub {
    /// The base URL of everything it asks.
    #[must_use]
    pub fn api(&self) -> &str {
        &self.api
    }

    /// The repository `options` name.
    #[must_use]
    pub fn new(options: &SourceOptions) -> Self {
        Self {
            api: options
                .api
                .as_deref()
                .unwrap_or(API)
                .trim_end_matches('/')
                .to_owned(),
            repository: options.repository.clone(),
        }
    }

    /// The event a `release` event's payload announces: a published release
    /// that is not a draft, or nothing.
    ///
    /// # Errors
    ///
    /// When the payload is too large, is not GitHub's release event, or
    /// makes no event.
    pub fn event(payload: &[u8]) -> Result<Option<Event>, ForgeError> {
        let payload: Payload = read(EVENT, payload)?;
        if payload.action != "published" {
            return Ok(None);
        }
        release(EVENT, &payload.repository, payload.release)
    }

    fn get(&self, path: &str, token: Option<&Secret>) -> Request {
        let request = Request::new(
            Method::Get,
            Text::plain(format!("{}/repos/{}{path}", self.api, self.repository)),
        )
        .header("Accept", Text::plain("application/vnd.github+json"))
        .header("X-GitHub-Api-Version", Text::plain(API_VERSION))
        .header("User-Agent", Text::plain(USER_AGENT));
        match token {
            Some(token) => request.header(
                "Authorization",
                Text::plain("Bearer ").then_secret(token.clone()),
            ),
            None => request,
        }
    }

    /// The request for the repository's numeric id and name.
    #[must_use]
    pub fn repository_request(&self, token: Option<&Secret>) -> Request {
        self.get("", token)
    }

    /// The repository GitHub's answer to [`GitHub::repository_request`]
    /// describes.
    ///
    /// # Errors
    ///
    /// When GitHub did not answer with the repository.
    pub fn repository(response: &Response) -> Result<Repository, ForgeError> {
        answered(REPOSITORY, response)?;
        read(REPOSITORY, &response.body)
    }

    /// The request for the newest releases, conditional on the validators
    /// `cursor` holds.
    #[must_use]
    pub fn releases_request(&self, cursor: Option<&Cursor>, token: Option<&Secret>) -> Request {
        let mut request = self.get(&format!("/releases?per_page={PAGE}"), token);
        if let Some(etag) = cursor.and_then(|cursor| cursor.etag.as_deref()) {
            request = request.header("If-None-Match", Text::plain(etag));
        }
        if let Some(modified) = cursor.and_then(|cursor| cursor.last_modified.as_deref()) {
            request = request.header("If-Modified-Since", Text::plain(modified));
        }
        request
    }

    /// The request for the release tagged `tag`.
    #[must_use]
    pub fn release_request(&self, tag: &str, token: Option<&Secret>) -> Request {
        let tag: String = tag
            .bytes()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
                    char::from(byte).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect();
        self.get(&format!("/releases/tags/{tag}"), token)
    }

    /// The event the release of `repository` in GitHub's answer to
    /// [`GitHub::release_request`] announces: a published release that is
    /// not a draft, or nothing.
    ///
    /// # Errors
    ///
    /// When GitHub did not answer with a release, or it makes no event.
    pub fn release(
        repository: &Repository,
        response: &Response,
    ) -> Result<Option<Event>, ForgeError> {
        answered(RELEASE, response)?;
        release(RELEASE, repository, read(RELEASE, &response.body)?)
    }

    /// Whether GitHub's answer to [`GitHub::releases_request`] says nothing
    /// changed since the cursor's validators.
    #[must_use]
    pub fn unchanged(response: &Response) -> bool {
        response.status == NOT_MODIFIED
    }

    /// The published releases of `repository` in GitHub's answer to
    /// [`GitHub::releases_request`].
    ///
    /// # Errors
    ///
    /// When GitHub did not answer with releases, or one makes no event.
    pub fn releases(repository: &Repository, response: &Response) -> Result<Releases, ForgeError> {
        answered(RELEASES, response)?;
        let releases: Vec<Release> = read(RELEASES, &response.body)?;
        let events = releases
            .into_iter()
            .filter_map(|found| release(RELEASES, repository, found).transpose())
            .collect::<Result<Vec<Event>, ForgeError>>()?;
        Ok(Releases {
            events,
            etag: response.header("etag").map(ToOwned::to_owned),
            last_modified: response.header("last-modified").map(ToOwned::to_owned),
        })
    }
}
