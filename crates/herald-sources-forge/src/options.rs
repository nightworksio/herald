use std::num::NonZeroUsize;

use herald_core::{SecretRef, Size, path_of_names, web_url};
use herald_wire::{PARSE_LIMIT, Reading};

use crate::Forge;

/// A forge source's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceOptions {
    /// The repository, as `owner/name`, or a GitLab project's full path,
    /// such as `group/subgroup/name`.
    pub repository: String,
    /// The API's base URL, for a forge that is not GitHub, GitLab.com or
    /// Codeberg: a GitHub Enterprise Server's `https://ghe.example/api/v3`,
    /// a GHE.com tenant's `https://api.acme.ghe.com`, a self-managed or
    /// Dedicated GitLab's `https://gitlab.example/api/v4`, or a Forgejo's
    /// `https://forge.example/api/v1`. Where absent, it is the CI run's own
    /// when announcing the run's repository, and otherwise the forge's
    /// public instance.
    pub api: Option<String>,
    /// A token that reaches a private repository or raises the API's rate
    /// limit, such as the run's `GITHUB_TOKEN`.
    pub token: Option<SecretRef>,
    /// How many of the newest releases the first poll announces; none where
    /// absent (ADR-0003).
    #[serde(default)]
    pub backfill: usize,
    /// The secret the forge signs its webhook deliveries to `herald server`
    /// with: GitHub's and Forgejo's webhook secret, or GitLab's signing
    /// token (ADR-0007).
    pub webhook: Option<SecretRef>,
    /// How many of the newest releases one poll reads; 30 where absent.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub page: Option<NonZeroUsize>,
    /// How many identities the cursor remembers, newest first; 500 where
    /// absent, and at least `page`.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub remembered: Option<NonZeroUsize>,
    /// The most of an answer parsed; "4MiB" where absent, and at most four
    /// times that.
    pub parse: Option<Size>,
}

impl SourceOptions {
    /// Whether the options are sound for a source on `forge`.
    ///
    /// # Errors
    ///
    /// When `repository` is not a path the forge names a repository by, or
    /// `api` is not a web address.
    pub fn check(&self, forge: Forge) -> Result<(), String> {
        if !path_of_names(&self.repository, forge.names()) {
            return Err(format!(
                "\"{}\" is not a repository; give it as {}, of letters, digits, '.', '_' and '-'",
                self.repository,
                forge.shape()
            ));
        }
        match &self.api {
            Some(api) if !web_url(api) => Err(format!(
                "\"{api}\" is not a web address; give the API's base URL, such as https://forge.example/api/v1"
            )),
            _ => self.reading().check(PARSE_LIMIT),
        }
    }

    /// How a poll of the source reads.
    #[must_use]
    pub fn reading(&self) -> Reading {
        Reading::of(self.page, self.remembered, self.parse, PARSE_LIMIT)
    }
}
