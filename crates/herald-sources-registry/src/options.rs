use std::num::NonZeroUsize;

use herald_core::{SecretRef, Size, path_of_names, web_url};
use herald_wire::Reading;

use crate::Registry;

/// How Docker Hub lists a repository's tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
pub enum TagOrdering {
    /// The most recently pushed first.
    #[default]
    #[serde(rename = "last_updated")]
    LastUpdated,
    /// By name.
    #[serde(rename = "name")]
    Name,
}

impl TagOrdering {
    /// The order as Docker Hub's `ordering` takes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LastUpdated => "last_updated",
            Self::Name => "name",
        }
    }
}

/// A registry source's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegistryOptions {
    /// The package, as the registry names it: `vendor/name` on Packagist,
    /// a crate's name, an npm package's name such as `@scope/name`, or a
    /// Docker Hub repository such as `library/caddy`, where an official
    /// image may leave out `library/`.
    #[schemars(example = "vendor/name")]
    pub package: String,
    /// The registry's base URL, for a mirror or a private registry that
    /// answers as the public one does; the public registry's where absent.
    pub api: Option<String>,
    /// A token for a private package: npm's, sent as a bearer token;
    /// Private Packagist's Composer token, sent as HTTP Basic with the user
    /// `token`; or a Docker Hub personal access token, exchanged with
    /// `username` for a bearer token. crates.io takes none.
    pub token: Option<SecretRef>,
    /// The Docker Hub account `token` belongs to.
    pub username: Option<String>,
    /// How Docker Hub lists a repository's tags for a poll:
    /// `last_updated`, the most recently pushed first, where unset, or
    /// `name`.
    pub ordering: Option<TagOrdering>,
    /// How many of the newest versions the first poll announces; none where
    /// absent (ADR-0003).
    #[serde(default)]
    pub backfill: usize,
    /// How many of the newest versions one poll reads; 30 where absent.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub page: Option<NonZeroUsize>,
    /// How many identities the cursor remembers, newest first; 500 where
    /// absent, and at least `page`.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub remembered: Option<NonZeroUsize>,
    /// The most of an answer parsed; "4MiB", "16MiB" on Packagist and "32MiB" on npm, where absent, and at most four
    /// times that.
    pub parse: Option<Size>,
}

impl RegistryOptions {
    /// Whether the options are sound for a source on `registry`.
    ///
    /// # Errors
    ///
    /// When `package` is not a name the registry gives a package, or `api`
    /// is not a web address.
    pub fn check(&self, registry: Registry) -> Result<(), String> {
        let (path, count) = match (registry, self.package.strip_prefix('@')) {
            (Registry::Npm, Some(scoped)) => (scoped, 2..=2),
            _ => (self.package.as_str(), registry.names()),
        };
        if !path_of_names(path, count) {
            return Err(format!(
                "\"{}\" is not a package; give it as {}, of letters, digits, '.', '_' and '-'",
                self.package,
                registry.shape()
            ));
        }
        match (registry.names_its_account(), &self.username, &self.token) {
            (true, None, Some(_)) | (true, Some(_), None) => {
                return Err(
                    "Docker Hub exchanges a personal access token with its account's username; give both token and username".to_owned(),
                );
            }
            (false, Some(_), _) => {
                return Err(format!(
                    "the registry \"{}\" takes no username; remove username",
                    registry.kind()
                ));
            }
            _ => {}
        }
        if self.ordering.is_some() && registry != Registry::DockerHub {
            return Err(format!(
                "the registry \"{}\" lists versions in its own order; remove ordering",
                registry.kind()
            ));
        }
        match &self.api {
            Some(api) if !web_url(api) => Err(format!(
                "\"{api}\" is not a web address; give the registry's base URL, such as https://registry.example"
            )),
            _ => self.reading(registry).check(registry.api().limit),
        }
    }

    /// How a poll of the package on `registry` reads.
    #[must_use]
    pub fn reading(&self, registry: Registry) -> Reading {
        Reading::of(self.page, self.remembered, self.parse, registry.api().limit)
    }
}
