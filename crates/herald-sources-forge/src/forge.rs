use herald_core::{Cursor, Event};
use herald_ports::{Method, Request, Response, Secret, Text, USER_AGENT};
use serde::Deserialize;

use crate::error::ForgeError;
use crate::options::SourceOptions;
use crate::wire::{Found, answered, event, read, segment};
use crate::{forgejo, github, gitlab};

/// The releases asked for in one poll.
const PAGE: u8 = 30;
/// What a forge answers to a conditional request when nothing changed.
const NOT_MODIFIED: u16 = 304;

/// A forge whose releases herald announces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forge {
    /// GitHub, or a GitHub Enterprise Server.
    GitHub,
    /// GitLab.com, or a self-managed GitLab.
    GitLab,
    /// Forgejo or Gitea, such as Codeberg.
    Forgejo,
}

/// How a forge's API is asked, and what each of its answers is called in
/// an error.
pub(crate) struct Api {
    /// The base URL of its public instance.
    pub(crate) url: &'static str,
    /// The query parameter that sizes a page.
    pub(crate) page: &'static str,
    /// The path to a release, before its tag.
    pub(crate) tagged: &'static str,
    pub(crate) event: &'static str,
    pub(crate) repository: &'static str,
    pub(crate) releases: &'static str,
    pub(crate) release: &'static str,
}

/// A repository, as its forge names it: its numeric id outlives a rename.
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
    /// The `ETag` the forge returned.
    pub etag: Option<String>,
    /// The `Last-Modified` the forge returned.
    pub last_modified: Option<String>,
}

/// The token a request carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// A token the config names.
    Named(Secret),
    /// The CI run's own token, such as `GITHUB_TOKEN` or GitLab's
    /// `CI_JOB_TOKEN`.
    Run(Secret),
}

/// One repository on one forge, whose releases are asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    forge: Forge,
    api: String,
    repository: String,
}

impl Forge {
    /// Its name, as a source's `kind` in `herald.toml` writes it.
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::GitLab => "gitlab",
            Self::Forgejo => "forgejo",
        }
    }

    const fn api(self) -> &'static Api {
        match self {
            Self::GitHub => &github::API,
            Self::GitLab => &gitlab::API,
            Self::Forgejo => &forgejo::API,
        }
    }

    /// The event a release event's payload announces, as a webhook or as
    /// the forge's CI hands it to a run: a published release that is not a
    /// draft, or nothing.
    ///
    /// # Errors
    ///
    /// When the payload is too large, is not the forge's release event, or
    /// makes no event.
    pub fn event(self, payload: &[u8]) -> Result<Option<Event>, ForgeError> {
        let what = self.api().event;
        let published = match self {
            Self::GitHub | Self::Forgejo => github::published(what, payload)?,
            Self::GitLab => gitlab::published(what, payload)?,
        };
        match published {
            Some((repository, found)) => event(what, &repository, found),
            None => Ok(None),
        }
    }

    /// The repository the forge's answer to [`Origin::repository_request`]
    /// describes.
    ///
    /// # Errors
    ///
    /// When the forge did not answer with the repository.
    pub fn repository(self, response: &Response) -> Result<Repository, ForgeError> {
        let what = self.api().repository;
        answered(what, response)?;
        read(what, &response.body)
    }

    fn found(self, what: &'static str, body: &[u8]) -> Result<Found, ForgeError> {
        match self {
            Self::GitHub | Self::Forgejo => github::found(what, body),
            Self::GitLab => gitlab::found(what, body),
        }
    }

    fn all_found(self, what: &'static str, body: &[u8]) -> Result<Vec<Found>, ForgeError> {
        match self {
            Self::GitHub | Self::Forgejo => github::all_found(what, body),
            Self::GitLab => gitlab::all_found(what, body),
        }
    }

    /// The event the release of `repository` in the forge's answer to
    /// [`Origin::release_request`] announces: a published release that is
    /// not a draft, or nothing.
    ///
    /// # Errors
    ///
    /// When the forge did not answer with a release, or it makes no event.
    pub fn release(
        self,
        repository: &Repository,
        response: &Response,
    ) -> Result<Option<Event>, ForgeError> {
        let what = self.api().release;
        answered(what, response)?;
        event(what, repository, self.found(what, &response.body)?)
    }

    /// The published releases of `repository` in the forge's answer to
    /// [`Origin::releases_request`].
    ///
    /// # Errors
    ///
    /// When the forge did not answer with releases, or one makes no event.
    pub fn releases(
        self,
        repository: &Repository,
        response: &Response,
    ) -> Result<Releases, ForgeError> {
        let what = self.api().releases;
        answered(what, response)?;
        let events = self
            .all_found(what, &response.body)?
            .into_iter()
            .filter_map(|found| event(what, repository, found).transpose())
            .collect::<Result<Vec<Event>, ForgeError>>()?;
        Ok(Releases {
            events,
            etag: response.header("etag").map(ToOwned::to_owned),
            last_modified: response.header("last-modified").map(ToOwned::to_owned),
        })
    }

    /// Whether the forge's answer to [`Origin::releases_request`] says
    /// nothing changed since the cursor's validators.
    #[must_use]
    pub fn unchanged(response: &Response) -> bool {
        response.status == NOT_MODIFIED
    }
}

impl Origin {
    /// The repository `options` name, on `forge`.
    #[must_use]
    pub fn new(forge: Forge, options: &SourceOptions) -> Self {
        let api = options.api.as_deref().unwrap_or(forge.api().url);
        Self {
            forge,
            api: api.trim_end_matches('/').to_owned(),
            repository: options.repository.clone(),
        }
    }

    /// The forge.
    #[must_use]
    pub const fn forge(&self) -> Forge {
        self.forge
    }

    /// The base URL of everything it asks.
    #[must_use]
    pub fn api(&self) -> &str {
        &self.api
    }

    fn get(&self, path: &str, token: Option<&Token>) -> Request {
        let base = match self.forge {
            Forge::GitHub | Forge::Forgejo => format!("{}/repos/{}", self.api, self.repository),
            Forge::GitLab => gitlab::project(&self.api, &self.repository),
        };
        let request = Request::new(Method::Get, Text::plain(format!("{base}{path}")));
        let request = match self.forge {
            Forge::GitHub => github::headed(request),
            Forge::GitLab | Forge::Forgejo => {
                request.header("Accept", Text::plain("application/json"))
            }
        }
        .header("User-Agent", Text::plain(USER_AGENT));
        match (self.forge, token) {
            (_, None) => request,
            (Forge::GitLab, Some(Token::Run(token))) => {
                request.header("JOB-TOKEN", Text::secret(token.clone()))
            }
            (Forge::Forgejo, Some(Token::Named(token) | Token::Run(token))) => request.header(
                "Authorization",
                Text::plain("token ").then_secret(token.clone()),
            ),
            (Forge::GitHub | Forge::GitLab, Some(Token::Named(token) | Token::Run(token))) => {
                request.header(
                    "Authorization",
                    Text::plain("Bearer ").then_secret(token.clone()),
                )
            }
        }
    }

    /// The request for the repository's numeric id and name.
    #[must_use]
    pub fn repository_request(&self, token: Option<&Token>) -> Request {
        self.get("", token)
    }

    /// The request for the newest releases, conditional on the validators
    /// `cursor` holds.
    #[must_use]
    pub fn releases_request(&self, cursor: Option<&Cursor>, token: Option<&Token>) -> Request {
        let page = self.forge.api().page;
        let mut request = self.get(&format!("/releases?{page}={PAGE}"), token);
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
    pub fn release_request(&self, tag: &str, token: Option<&Token>) -> Request {
        let path = format!("{}{}", self.forge.api().tagged, segment(tag));
        self.get(&path, token)
    }
}
