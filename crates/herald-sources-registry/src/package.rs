use std::collections::BTreeMap;

use herald_core::{Cursor, Draft, Event, Kind, Polled, prerelease_version};
use herald_ports::{Method, Request, Response, Secret, Text, USER_AGENT};
use herald_wire::{PAGE, SourceError, answered, host};
use jiff::Timestamp;

use crate::options::RegistryOptions;
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
}

impl Registry {
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

    const fn api(self) -> &'static Api {
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
        matches!(self, Self::Npm)
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
        }
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

    /// The request for the newest versions, conditional on the validators
    /// `cursor` holds, carrying `token` where the registry takes one.
    #[must_use]
    pub fn versions_request(&self, cursor: Option<&Cursor>, token: Option<&Secret>) -> Request {
        let path = match self.registry {
            Registry::Packagist => packagist::path(&self.name),
            Registry::CratesIo => crates_io::path(&self.name),
            Registry::Npm => npm::path(&self.name),
            Registry::DockerHub => docker_hub::path(&self.name),
        };
        let mut request = Request::new(Method::Get, Text::plain(format!("{}{path}", self.api)))
            .header("Accept", Text::plain("application/json"))
            .header("User-Agent", Text::plain(USER_AGENT));
        if let Some(token) = token.filter(|_| self.registry.takes_token()) {
            request = request.header(
                "Authorization",
                Text::plain("Bearer ").then_secret(token.clone()),
            );
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
            Registry::Packagist => packagist::found(api, &self.name, body)?,
            Registry::CratesIo => crates_io::found(api, body)?,
            Registry::Npm => npm::found(api, body)?,
            Registry::DockerHub => docker_hub::found(api, body)?,
        };
        found.sort_by_key(|found| std::cmp::Reverse(found.published));
        found.truncate(PAGE);
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
            links.insert("release".to_owned(), page);
        }
        let draft = Draft {
            identity: format!("{}/{}/{}", host(&self.api), self.name, found.version),
            kind: Kind::PackageVersion,
            project: project.to_owned(),
            prerelease: prerelease_version(&found.version),
            title: format!("{project} {}", found.version),
            version: Some(found.version),
            body: String::new(),
            links,
            occurred: found.published,
        };
        Event::normalise(draft).map_err(|problem| SourceError::Invalid { what, problem })
    }
}
