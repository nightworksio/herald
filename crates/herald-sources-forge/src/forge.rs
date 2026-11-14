use herald_core::{Cursor, Event, KeyForm, Polled, Scheme, Signed, unreserved};
use herald_core::{media, status};
use herald_kinds::Hooked;
use herald_ports::{Method, Request, Response, Secret, SecretText};
use serde::Deserialize;

use crate::options::SourceOptions;
use herald_wire::SourceError;
use herald_wire::{Reading, answered};

use crate::wire::{ForgeRelease, event, read};
use crate::{forgejo, github, gitlab};

/// A GitHub webhook: `X-Hub-Signature-256: sha256=<hex>`, an HMAC-SHA256 of the body.
const GITHUB: Scheme = Scheme {
    name: "github",
    signed: Signed::Body {
        headers: &["X-Hub-Signature-256"],
        prefix: "sha256=",
    },
    key: KeyForm::Bytes,
};

/// A GitLab webhook signed with a signing token, as Standard Webhooks sign.
const GITLAB: Scheme = Scheme {
    name: "gitlab",
    signed: Signed::StandardWebhooks {
        id: "webhook-id",
        timestamp: "webhook-timestamp",
        signature: "webhook-signature",
    },
    key: KeyForm::Whsec,
};

/// A Forgejo or Gitea webhook: `X-Forgejo-Signature` or `X-Gitea-Signature`, the hex HMAC-SHA256 of the body.
const FORGEJO: Scheme = Scheme {
    name: "forgejo",
    signed: Signed::Body {
        headers: &["X-Forgejo-Signature", "X-Gitea-Signature"],
        prefix: "",
    },
    key: KeyForm::Bytes,
};

/// Why a GitLab webhook may go unserved (ADR-0007).
const UNSIGNED: &str = "GitLab signs a webhook only with a signing token, which an older self-managed instance does not offer; where it cannot sign, remove webhook and let herald poll the project.";

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

/// The most names a GitLab project's path holds: its top group, 20
/// subgroups and its own name.
const GITLAB_DEPTH: usize = 22;

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

/// Why a webhook delivery of another event announces nothing.
const OTHER_EVENT: &str = "the delivery is not of a release";
/// Why a webhook delivery of an unpublished release announces nothing.
const UNPUBLISHED: &str = "the release is not published, or is a draft";

/// A repository, as its forge names it: its numeric id outlives a rename.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Repository {
    /// The numeric id.
    pub id: u64,
    /// The name, without its owner.
    pub name: String,
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
    reading: Reading,
}

impl Forge {
    /// Who signs its webhooks, and how.
    #[must_use]
    pub const fn scheme(self) -> Scheme {
        match self {
            Self::GitHub => GITHUB,
            Self::GitLab => GITLAB,
            Self::Forgejo => FORGEJO,
        }
    }

    /// What a webhook from the forge leaves to it, where anything.
    #[must_use]
    pub const fn unsigned(self) -> Option<&'static str> {
        match self {
            Self::GitLab => Some(UNSIGNED),
            Self::GitHub | Self::Forgejo => None,
        }
    }

    /// How many names a repository's path holds on the forge.
    #[must_use]
    pub const fn names(self) -> std::ops::RangeInclusive<usize> {
        match self {
            Self::GitHub | Self::Forgejo => 2..=2,
            Self::GitLab => 2..=GITLAB_DEPTH,
        }
    }

    /// How a repository is written on the forge.
    #[must_use]
    pub const fn shape(self) -> &'static str {
        match self {
            Self::GitHub | Self::Forgejo => "owner/name",
            Self::GitLab => "the project's full path, such as group/subgroup/name",
        }
    }

    /// Every forge.
    pub const ALL: [Self; 3] = [Self::GitHub, Self::GitLab, Self::Forgejo];

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

    /// What a webhook delivery with `headers` and `payload` asks herald to
    /// announce for the source of `repository`: the release it publishes,
    /// or nothing where it is of another event, or of an unpublished
    /// release or a draft.
    ///
    /// # Errors
    ///
    /// When the payload is too large, is not the release event its headers
    /// name, makes no event, or is of another repository than `repository`.
    pub fn hooked(
        self,
        headers: &[(&str, &str)],
        payload: &[u8],
        repository: &str,
    ) -> Result<Hooked, SourceError> {
        let (names, release): (&[&str], &str) = match self {
            Self::GitHub => (&["x-github-event"], "release"),
            Self::Forgejo => (&["x-forgejo-event", "x-gitea-event"], "release"),
            Self::GitLab => (&["x-gitlab-event"], "Release Hook"),
        };
        let kind = headers
            .iter()
            .find(|(name, _)| names.iter().any(|header| name.eq_ignore_ascii_case(header)))
            .map(|(_, value)| *value);
        if kind != Some(release) {
            return Ok(Hooked::Nothing(OTHER_EVENT));
        }
        let event = self.event(payload)?;
        let what = self.api().event;
        let path = match self {
            Self::GitHub | Self::Forgejo => github::path(what, payload),
            Self::GitLab => gitlab::path(what, payload),
        };
        if !path.is_some_and(|path| path.eq_ignore_ascii_case(repository)) {
            return Err(SourceError::Unreadable {
                what,
                reason: format!("it is not of {repository}, the repository its source names"),
            });
        }
        Ok(event.map_or(Hooked::Nothing(UNPUBLISHED), Hooked::Event))
    }

    /// The event a release event's payload announces, as a webhook or as
    /// the forge's CI hands it to a run: a published release that is not a
    /// draft, or nothing.
    ///
    /// # Errors
    ///
    /// When the payload is too large, is not the forge's release event, or
    /// makes no event.
    pub fn event(self, payload: &[u8]) -> Result<Option<Event>, SourceError> {
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
    pub fn repository(self, response: &Response) -> Result<Repository, SourceError> {
        let what = self.api().repository;
        answered(what, response)?;
        read(what, &response.body)
    }

    fn found(self, what: &'static str, body: &[u8]) -> Result<ForgeRelease, SourceError> {
        match self {
            Self::GitHub | Self::Forgejo => github::found(what, body),
            Self::GitLab => gitlab::found(what, body),
        }
    }

    fn all_found(
        self,
        what: &'static str,
        limit: usize,
        body: &[u8],
    ) -> Result<Vec<ForgeRelease>, SourceError> {
        match self {
            Self::GitHub | Self::Forgejo => github::all_found(what, limit, body),
            Self::GitLab => gitlab::all_found(what, limit, body),
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
    ) -> Result<Option<Event>, SourceError> {
        let what = self.api().release;
        answered(what, response)?;
        event(what, repository, self.found(what, &response.body)?)
    }

    /// The published releases of `repository` in the forge's answer to
    /// [`Origin::releases_request`], parsing no more than `limit` bytes.
    ///
    /// # Errors
    ///
    /// When the forge did not answer with releases, or one makes no event.
    pub fn releases(
        self,
        repository: &Repository,
        response: &Response,
        limit: usize,
    ) -> Result<Polled, SourceError> {
        let what = self.api().releases;
        answered(what, response)?;
        let events = self
            .all_found(what, limit, &response.body)?
            .into_iter()
            .filter_map(|found| event(what, repository, found).transpose())
            .collect::<Result<Vec<Event>, SourceError>>()?;
        Ok(Polled {
            events,
            etag: response.header("etag").map(ToOwned::to_owned),
            last_modified: response.header("last-modified").map(ToOwned::to_owned),
        })
    }

    /// Whether the forge's answer to [`Origin::releases_request`] says
    /// nothing changed since the cursor's validators.
    #[must_use]
    pub fn unchanged(response: &Response) -> bool {
        response.status == status::NOT_MODIFIED
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
            reading: options.reading(),
        }
    }

    /// How a poll of it reads.
    #[must_use]
    pub const fn reading(&self) -> Reading {
        self.reading
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
        let request = Request::new(Method::Get, SecretText::plain(format!("{base}{path}")))
            .at_most(self.reading.parse);
        let request = match self.forge {
            Forge::GitHub => herald_wire::github::headed(request),
            Forge::GitLab | Forge::Forgejo => request
                .header("Accept", SecretText::plain(media::JSON))
                .identified(),
        };
        match (self.forge, token) {
            (_, None) => request,
            (Forge::GitLab, Some(Token::Run(token))) => {
                request.header("JOB-TOKEN", SecretText::secret(token.clone()))
            }
            (Forge::Forgejo, Some(Token::Named(token) | Token::Run(token))) => request.header(
                "Authorization",
                SecretText::plain("token ").then_secret(token.clone()),
            ),
            (Forge::GitHub | Forge::GitLab, Some(Token::Named(token) | Token::Run(token))) => {
                request.bearer(token.clone())
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
        let mut request = self.get(&format!("/releases?{page}={}", self.reading.page), token);
        if let Some(etag) = cursor.and_then(|cursor| cursor.etag.as_deref()) {
            request = request.header("If-None-Match", SecretText::plain(etag));
        }
        if let Some(modified) = cursor.and_then(|cursor| cursor.last_modified.as_deref()) {
            request = request.header("If-Modified-Since", SecretText::plain(modified));
        }
        request
    }

    /// The request for the release tagged `tag`.
    #[must_use]
    pub fn release_request(&self, tag: &str, token: Option<&Token>) -> Request {
        let path = format!("{}{}", self.forge.api().tagged, unreserved(tag));
        self.get(&path, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_forge_signs_under_its_own_scheme() {
        assert_eq!(
            [Forge::GitHub, Forge::GitLab, Forge::Forgejo].map(|forge| forge.scheme().name),
            ["github", "gitlab", "forgejo"]
        );
    }

    #[test]
    fn each_forge_is_named_as_a_source_s_kind_writes_it() {
        assert_eq!(Forge::ALL.map(Forge::kind), ["github", "gitlab", "forgejo"]);
    }

    #[test]
    fn each_forge_takes_a_repository_in_its_shape_and_an_api_at_a_web_address() {
        let options = |repository: &str, api: Option<&str>| crate::SourceOptions {
            repository: repository.to_owned(),
            api: api.map(ToOwned::to_owned),
            ..crate::SourceOptions::default()
        };
        for forge in Forge::ALL {
            assert_eq!(
                options("nightworksio/herald", Some("https://forge.example/api/v1")).check(forge),
                Ok(())
            );
            assert_eq!(
                options("a/b", Some("ftp://forge.example")).check(forge),
                Err("\"ftp://forge.example\" is not a web address; give the API's base URL, such as https://forge.example/api/v1".to_owned())
            );
        }
        assert_eq!(options("group/sub/name", None).check(Forge::GitLab), Ok(()));
        assert!(
            options("group/sub/name", None)
                .check(Forge::GitHub)
                .is_err()
        );
        for (forge, shape) in [
            (Forge::GitHub, "owner/name"),
            (
                Forge::GitLab,
                "the project's full path, such as group/subgroup/name",
            ),
            (Forge::Forgejo, "owner/name"),
        ] {
            assert_eq!(
                options("a/../b", None).check(forge),
                Err(format!(
                    "\"a/../b\" is not a repository; give it as {shape}, of letters, digits, '.', '_' and '-'"
                ))
            );
        }
    }
}
