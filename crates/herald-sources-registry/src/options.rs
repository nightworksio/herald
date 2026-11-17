use std::num::NonZeroUsize;

use herald_core::{Duration, SecretRef, Size, web_url};
use herald_wire::Reading;

use crate::Registry;

mod namespace;

pub(crate) use namespace::CratesOwner;
pub use namespace::{Declared, VendorListing};

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
    pub package: Option<String>,
    /// The npm scope whose packages the source takes, each polled on its
    /// own, such as `@acme`; exclusive with `package`.
    #[schemars(example = "@acme")]
    pub scope: Option<String>,
    /// The Packagist vendor whose packages the source takes, each polled
    /// on its own; exclusive with `package`.
    pub vendor: Option<String>,
    /// The crates.io owner whose crates the source takes, each polled on
    /// its own, as crates.io writes owners: `user:<login>` or
    /// `team:github:<org>:<team>`; exclusive with `package`.
    #[schemars(example = "user:<login>")]
    pub owner: Option<String>,
    /// The Docker Hub namespace whose repositories the source takes, each
    /// polled on its own; exclusive with `package`.
    pub namespace: Option<String>,
    /// Globs of the package names a namespace source takes; every name
    /// where empty.
    #[serde(default)]
    pub include: Vec<String>,
    /// Globs of the package names a namespace source leaves out, whatever
    /// `include` says.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// The most packages a namespace's listing reads before `include` and
    /// `exclude`; 10000 where absent. A namespace listing more is refused
    /// for the round.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub most_listed: Option<NonZeroUsize>,
    /// The most packages `include` and `exclude` may keep; 1000 where
    /// absent, and at most 100000. A namespace keeping more is refused for
    /// the round.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub most_members: Option<NonZeroUsize>,
    /// How long a package that left the namespace is kept, with its
    /// cursor, before it is forgotten; "30d" where absent.
    pub keep_members: Option<Duration>,
    /// How many packages one page of a namespace's listing asks for, where
    /// the registry pages it; the most it allows where absent.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub listed_page: Option<NonZeroUsize>,
    /// Who may see a namespace's packages where its listing does not say:
    /// `public` declares them public. Where absent, they are public when
    /// the source has no token and private when it has one, which a token
    /// may list.
    pub visibility: Option<Declared>,
    /// Whether an npm scope source also takes the packages its
    /// organization owns outside the scope.
    #[serde(default)]
    pub unscoped: bool,
    /// How a Packagist vendor's packages are listed: `packagist`, through
    /// `/packages/list.json` at `listing_api`, where absent, or
    /// `composer`, through a Composer repository's `packages.json` at
    /// `api`.
    pub listing: Option<VendorListing>,
    /// The base URL serving Packagist's `/packages/list.json` for a vendor
    /// source; `https://packagist.org` where absent.
    pub listing_api: Option<String>,
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
        self.namespace_check(registry)?;
        if let Some(package) = self
            .package
            .as_ref()
            .filter(|package| !registry.writes_package(package))
        {
            return Err(format!(
                "\"{package}\" is not a package; give it as {}, of letters, digits, '.', '_' and '-'",
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
