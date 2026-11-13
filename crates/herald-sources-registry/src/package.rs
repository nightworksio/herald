use std::collections::BTreeMap;

use herald_core::{
    Cursor, Draft, Event, Kind, Polled, RELEASE_LINK, prerelease_version, versioned,
};
use herald_ports::{Method, Request, Response, Secret, Text, USER_AGENT};
use herald_wire::{Reading, SourceError, answered};
use jiff::Timestamp;

use crate::options::{RegistryOptions, TagOrdering};
use crate::{crates_io, docker_hub, npm, packagist};

/// A registry whose package versions herald announces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registry {
    /// Packagist, PHP's registry.
    Packagist,
    /// crates.io, Rust's registry.
    CratesIo,
    /// The npm registry.
    Npm,
    /// Docker Hub, whose tags are a repository's versions.
    DockerHub,
}

/// How a registry is asked, and what its answer is called in an error.
pub(crate) struct Api {
    /// The base URL of the public registry.
    pub(crate) url: &'static str,
    /// The most bytes its answer may hold.
    pub(crate) limit: usize,
    /// What its answer is called.
    pub(crate) versions: &'static str,
}

/// A version, as a registry describes it.
pub(crate) struct Found {
    pub(crate) version: String,
    pub(crate) published: Timestamp,
}

/// One package on one registry, whose versions are asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    registry: Registry,
    api: String,
    name: String,
    reading: Reading,
    username: Option<String>,
    ordering: TagOrdering,
}

impl Registry {
    /// How many names a package's name holds on the registry.
    #[must_use]
    pub const fn names(self) -> std::ops::RangeInclusive<usize> {
        match self {
            Self::Packagist => 2..=2,
            Self::CratesIo | Self::Npm => 1..=1,
            Self::DockerHub => 1..=2,
        }
    }

    /// How a package is written on the registry.
    #[must_use]
    pub const fn shape(self) -> &'static str {
        match self {
            Self::Packagist => "vendor/name",
            Self::CratesIo => "the crate's name",
            Self::Npm => "the package's name, or @scope/name",
            Self::DockerHub => "the repository, such as library/caddy or caddy",
        }
    }

    /// Every registry.
    pub const ALL: [Self; 4] = [Self::Packagist, Self::CratesIo, Self::Npm, Self::DockerHub];

    /// Its name, as a source's `kind` in `herald.toml` writes it.
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Packagist => "packagist",
            Self::CratesIo => "crates-io",
            Self::Npm => "npm",
            Self::DockerHub => "docker-hub",
        }
    }

    pub(crate) const fn api(self) -> &'static Api {
        match self {
            Self::Packagist => &packagist::API,
            Self::CratesIo => &crates_io::API,
            Self::Npm => &npm::API,
            Self::DockerHub => &docker_hub::API,
        }
    }

    /// Whether the registry takes a token.
    #[must_use]
    pub const fn takes_token(self) -> bool {
        matches!(self, Self::Npm | Self::Packagist | Self::DockerHub)
    }

    /// Whether its token belongs to an account the source names: Docker
    /// Hub's personal access tokens are exchanged with their username.
    #[must_use]
    pub const fn names_its_account(self) -> bool {
        matches!(self, Self::DockerHub)
    }

    /// Whether the registry's versions include tags that move, such as
    /// Docker Hub's `latest`.
    #[must_use]
    pub const fn moving_tags(self) -> bool {
        matches!(self, Self::DockerHub)
    }

    /// Whether the registry's answer to [`Package::versions_request`] says
    /// nothing changed since the cursor's validators.
    #[must_use]
    pub fn unchanged(response: &Response) -> bool {
        response.status == herald_wire::NOT_MODIFIED
    }
}

impl Package {
    /// The package `options` name, on `registry`.
    #[must_use]
    pub fn new(registry: Registry, options: &RegistryOptions) -> Self {
        let name = match registry {
            Registry::DockerHub if !options.package.contains('/') => {
                format!("{}/{}", docker_hub::OFFICIAL, options.package)
            }
            _ => options.package.clone(),
        };
        Self {
            registry,
            api: options
                .api
                .as_deref()
                .unwrap_or(registry.api().url)
                .trim_end_matches('/')
                .to_owned(),
            name,
            reading: options.reading(registry),
            username: options.username.clone(),
            ordering: options.ordering.unwrap_or_default(),
        }
    }

    /// The request that exchanges `token` for the short-lived bearer token
    /// the registry's API takes, where the registry exchanges one: Docker
    /// Hub's, sent with the account's username in a sealed body.
    #[must_use]
    pub fn token_request(&self, token: &Secret) -> Option<Request> {
        let username = self.username.as_deref()?;
        self.registry.names_its_account().then(|| {
            let body = serde_json::json!({ "identifier": username, "secret": token.expose() });
            Request::new(
                Method::Post,
                Text::plain(format!("{}/auth/token", self.api)),
            )
            .at_most(docker_hub::TOKEN_ANSWER)
            .header("Content-Type", Text::plain("application/json"))
            .header("User-Agent", Text::plain(USER_AGENT))
            .sealed_body(body.to_string())
        })
    }

    /// The bearer token the registry answered `response` with, named as
    /// `token`, the secret it was exchanged for.
    ///
    /// # Errors
    ///
    /// When the registry refused the exchange or answered no token.
    pub fn exchanged(&self, token: &Secret, response: &Response) -> Result<Secret, SourceError> {
        docker_hub::exchanged(response)
            .map(|access| Secret::new(token.name(), access))
            .ok_or_else(|| SourceError::Unreadable {
                what: "Docker Hub's access token",
                reason: format!(
                    "Docker Hub answered {} to the exchange of {}; check the username and that the token may read the repository",
                    response.status,
                    token.name()
                ),
            })
    }

    /// How a poll of it reads.
    #[must_use]
    pub const fn reading(&self) -> Reading {
        self.reading
    }

    /// The registry.
    #[must_use]
    pub const fn registry(&self) -> Registry {
        self.registry
    }

    /// The base URL of everything it asks.
    #[must_use]
    pub fn api(&self) -> &str {
        &self.api
    }

    /// The package's name, as the registry names it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The name its events give as their project: the package's, without
    /// Docker Hub's `library/` for an official image.
    fn project(&self) -> &str {
        match self.registry {
            Registry::DockerHub => docker_hub::project(&self.name),
            _ => &self.name,
        }
    }

    fn public(&self) -> bool {
        self.api == self.registry.api().url
    }

    /// How `token` authorises a request to the registry: as HTTP Basic
    /// with the user `token` on Packagist, as Private Packagist takes it,
    /// and as a bearer token elsewhere.
    fn authorization(&self, token: &Secret) -> Text {
        match self.registry {
            Registry::Packagist => Text::plain("Basic ").then_secret(Secret::new(
                token.name(),
                herald_core::basic("token", token.expose()),
            )),
            _ => Text::plain("Bearer ").then_secret(token.clone()),
        }
    }

    /// The request for the newest versions, conditional on the validators
    /// `cursor` holds, carrying `token` where the registry takes one.
    #[must_use]
    pub fn versions_request(&self, cursor: Option<&Cursor>, token: Option<&Secret>) -> Request {
        let path = match self.registry {
            Registry::Packagist => packagist::path(&self.name),
            Registry::CratesIo => crates_io::path(&self.name, self.reading.page),
            Registry::Npm => npm::path(&self.name),
            Registry::DockerHub => docker_hub::path(&self.name, self.reading.page, self.ordering),
        };
        let mut request = Request::new(Method::Get, Text::plain(format!("{}{path}", self.api)))
            .at_most(self.reading.parse)
            .header("Accept", Text::plain("application/json"))
            .header("User-Agent", Text::plain(USER_AGENT));
        if let Some(token) = token.filter(|_| self.registry.takes_token()) {
            request = request.header("Authorization", self.authorization(token));
        }
        if let Some(etag) = cursor.and_then(|cursor| cursor.etag.as_deref()) {
            request = request.header("If-None-Match", Text::plain(etag));
        }
        if let Some(modified) = cursor.and_then(|cursor| cursor.last_modified.as_deref()) {
            request = request.header("If-Modified-Since", Text::plain(modified));
        }
        request
    }

    /// The newest versions in the registry's answer to
    /// [`Package::versions_request`], the newest first.
    ///
    /// # Errors
    ///
    /// When the registry did not answer with the package's versions, or one
    /// makes no event.
    pub fn versions(&self, response: &Response) -> Result<Polled, SourceError> {
        let api = self.registry.api();
        answered(api.versions, response)?;
        let body = &response.body;
        let mut found = match self.registry {
            Registry::Packagist => packagist::found(api, self.reading.parse, &self.name, body)?,
            Registry::CratesIo => crates_io::found(api, self.reading.parse, body)?,
            Registry::Npm => npm::found(api, self.reading.parse, body)?,
            Registry::DockerHub => docker_hub::found(api, self.reading.parse, body)?,
        };
        found.sort_by_key(|found| std::cmp::Reverse(found.published));
        found.truncate(self.reading.page);
        let events = found
            .into_iter()
            .map(|found| self.event(api.versions, found))
            .collect::<Result<Vec<Event>, SourceError>>()?;
        Ok(Polled {
            events,
            etag: response.header("etag").map(ToOwned::to_owned),
            last_modified: response.header("last-modified").map(ToOwned::to_owned),
        })
    }

    fn event(&self, what: &'static str, found: Found) -> Result<Event, SourceError> {
        let project = self.project();
        let mut links = BTreeMap::new();
        if self.public() {
            let page = match self.registry {
                Registry::Packagist => packagist::page(&self.name, &found.version),
                Registry::CratesIo => crates_io::page(&self.name, &found.version),
                Registry::Npm => npm::page(&self.name, &found.version),
                Registry::DockerHub => docker_hub::page(&self.name, &found.version),
            };
            links.insert(RELEASE_LINK.to_owned(), page);
        }
        let identity = format!("{}/{}/{}", self.registry.kind(), self.name, found.version);
        let title = format!("{project} {}", found.version);
        let prerelease = prerelease_version(&found.version);
        let draft = Draft {
            identity,
            kind: Kind::PACKAGE_VERSION,
            project: project.to_owned(),
            title,
            attributes: versioned(found.version, prerelease),
            body: String::new(),
            links,
            occurred: found.published,
        };
        Event::normalise(draft).map_err(|problem| SourceError::Invalid { what, problem })
    }
}
