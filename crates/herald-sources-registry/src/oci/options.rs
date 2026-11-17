use std::num::NonZeroUsize;

use herald_core::{
    Address, Duration, Glob, KEEP_MEMBERS, MOST_LISTED, MOST_MEMBERS, MOST_MEMBERS_ALLOWED,
    SecretRef, Selection, Size, Visibility, path_of_names, web_url,
};
use herald_wire::Reading;
use jiff::SignedDuration;

use super::session::origin_of;
use crate::Declared;
use crate::docker_hub;

/// The most tags a repository's listing reads where `most_tags` is absent.
pub(crate) const MOST_TAGS: NonZeroUsize = NonZeroUsize::new(10_000).unwrap();

/// The most `most_tags` may be.
pub(crate) const MOST_TAGS_ALLOWED: usize = 100_000;

/// How many tags or repositories one page of a listing asks for where
/// `listed_page` is absent.
pub(crate) const LISTED_PAGE: usize = 1_000;

/// The most `listed_page` may be.
pub(crate) const LISTED_PAGE_ALLOWED: usize = 10_000;

/// The hosts Docker Hub's registry answers at.
const DOCKER_HUB: [&str; 4] = [
    "registry-1.docker.io",
    "index.docker.io",
    "registry.hub.docker.com",
    docker_hub::REGISTRY,
];

/// Where Docker Hub's registry sends its tokens from.
const DOCKER_HUB_AUTH: &str = "https://auth.docker.io";

/// Where Docker Hub's registry redirects a blob to.
const DOCKER_HUB_BLOBS: [&str; 2] = [
    "https://production.cloudfront.docker.com",
    "https://production.cloudflare.docker.com",
];

/// GitHub's container registry.
const GHCR: &str = "ghcr.io";

/// Where GitHub's container registry redirects a blob to.
const GHCR_BLOBS: [&str; 1] = ["https://pkg-containers.githubusercontent.com"];

/// The tags that move where `moving` is absent.
const MOVING: [&str; 1] = ["latest"];

/// The keys only a catalog source may give.
const CATALOGUED: [&str; 5] = [
    "include",
    "exclude",
    "most_listed",
    "most_members",
    "keep_members",
];

/// An `oci` source's options in `herald.toml` (ADR-0042).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OciOptions {
    /// The registry's base URL, which serves the OCI Distribution API under
    /// `/v2/`, such as `https://registry-1.docker.io`, `https://ghcr.io` or
    /// `https://registry.gitlab.com`.
    #[schemars(example = "https://registry.example")]
    pub registry: String,
    /// The repository whose tags the source takes, such as `acme/app`; on
    /// Docker Hub an official image may leave out `library/`. Exclusive
    /// with `catalog`.
    #[schemars(example = "acme/app")]
    pub repository: Option<String>,
    /// Whether the source takes every repository the registry's
    /// `/v2/_catalog` lists, each polled on its own. Exclusive with
    /// `repository`.
    #[serde(default)]
    pub catalog: bool,
    /// Globs of the repositories a catalog source takes; every one where
    /// empty.
    #[serde(default)]
    pub include: Vec<String>,
    /// Globs of the repositories a catalog source leaves out, whatever
    /// `include` says.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// The most repositories the catalog reads before `include` and
    /// `exclude`; 10000 where absent. A catalog listing more is refused for
    /// the round.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub most_listed: Option<NonZeroUsize>,
    /// The most repositories `include` and `exclude` may keep; 1000 where
    /// absent, and at most 100000. A catalog keeping more is refused for
    /// the round.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub most_members: Option<NonZeroUsize>,
    /// How long a repository that left the catalog is kept, with its
    /// cursor, before it is forgotten; "30d" where absent.
    pub keep_members: Option<Duration>,
    /// How many tags, or repositories of the catalog, one page of a listing
    /// asks for; 1000 where absent, and at most 10000.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub listed_page: Option<NonZeroUsize>,
    /// The most tags a repository's listing reads; at most 100000. A
    /// repository listing more is refused for the round. The cursor
    /// remembers every tag, so where absent it is 10000, or as many as the
    /// store keeps in one record where that is fewer, which the doctor
    /// names; a value the store's records cannot hold is refused.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub most_tags: Option<NonZeroUsize>,
    /// The base URL of the one origin besides the registry's own that may
    /// hand out its tokens: a `Bearer` challenge naming a realm anywhere
    /// else is refused. `https://auth.docker.io` for Docker Hub's registry
    /// where absent, and none otherwise; GitLab's registry needs its
    /// GitLab's, such as `https://gitlab.com`.
    pub auth: Option<String>,
    /// The base URLs of the origins a config blob may be redirected to,
    /// fetched without the registry's credential. Docker Hub's and
    /// GitHub's CDNs for their registries where absent, and none otherwise.
    /// A blob redirected elsewhere is not read: its tag takes the time it
    /// was first seen.
    pub blobs: Option<Vec<String>>,
    /// The registry's name in each tag's identity, such as `docker.io` for
    /// a mirror of Docker Hub, so the tag is announced once with any other
    /// source of the image; `docker.io` for Docker Hub's registry where
    /// absent, and the registry's host otherwise.
    #[schemars(example = "docker.io")]
    pub registry_name: Option<String>,
    /// Globs of the tags that move, such as `latest`, which are never
    /// announced; `["latest"]` where absent.
    pub moving: Option<Vec<String>>,
    /// The account `token` belongs to, sent with it as HTTP Basic to the
    /// registry's token service, or to the registry where it asks for
    /// Basic.
    pub username: Option<String>,
    /// A token or password for a private repository, sent with `username`.
    /// Anonymous where absent.
    pub token: Option<SecretRef>,
    /// Who may see the repositories: `public` declares them public. Where
    /// absent, they are public when the source has no token and private
    /// when it has one.
    pub visibility: Option<Declared>,
    /// How many of the newest tags the first poll announces; none where
    /// absent. The registry gives tags no time, so the first poll reads
    /// the last `page` tags its listing names to find the newest.
    #[serde(default)]
    pub backfill: usize,
    /// How many unseen tags one poll reads; 30 where absent. Each takes up
    /// to four requests.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub page: Option<NonZeroUsize>,
    /// How many identities the cursor remembers; `most_tags` where absent,
    /// and at least that.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub remembered: Option<NonZeroUsize>,
    /// The most of an answer parsed; "4MiB" where absent, and at most four
    /// times that.
    pub parse: Option<Size>,
}

impl OciOptions {
    /// The registry's base URL, without a trailing slash.
    #[must_use]
    pub fn registry(&self) -> String {
        self.registry.trim_end_matches('/').to_owned()
    }

    /// The registry's host and port, where `registry` names a host.
    #[must_use]
    pub fn host(&self) -> String {
        Address::parse(&self.registry)
            .map(|address| address.authority())
            .unwrap_or_default()
    }

    /// Whether the registry is Docker Hub's.
    #[must_use]
    pub fn docker_hub(&self) -> bool {
        DOCKER_HUB.contains(&self.host().as_str())
    }

    /// The base URL of the one origin besides the registry's that may hand
    /// out its tokens, where there is one.
    #[must_use]
    pub fn auth(&self) -> Option<String> {
        self.auth
            .as_deref()
            .or_else(|| self.docker_hub().then_some(DOCKER_HUB_AUTH))
            .map(|auth| auth.trim_end_matches('/').to_owned())
    }

    /// The base URLs of the origins a config blob may be redirected to.
    #[must_use]
    pub fn blobs(&self) -> Vec<String> {
        self.blobs.clone().unwrap_or_else(|| {
            let defaults: &[&str] = if self.docker_hub() {
                &DOCKER_HUB_BLOBS
            } else if self.host() == GHCR {
                &GHCR_BLOBS
            } else {
                &[]
            };
            defaults.iter().map(|&blob| blob.to_owned()).collect()
        })
    }

    /// The registry's name in each tag's identity.
    #[must_use]
    pub fn registry_name(&self) -> String {
        self.registry_name
            .clone()
            .unwrap_or_else(|| {
                if self.docker_hub() {
                    docker_hub::REGISTRY.to_owned()
                } else {
                    self.host()
                }
            })
            .to_ascii_lowercase()
    }

    /// `repository` as the registry names it: on Docker Hub, an official
    /// image under `library/`.
    #[must_use]
    pub fn repository_of(&self, repository: &str) -> String {
        if self.docker_hub() && !repository.contains('/') {
            format!("{}/{repository}", docker_hub::OFFICIAL)
        } else {
            repository.to_owned()
        }
    }

    /// The tags that move.
    ///
    /// # Errors
    ///
    /// When `moving` holds a glob that is not one.
    pub fn moving(&self) -> Result<Vec<Glob>, String> {
        let texts: Vec<&str> = self.moving.as_ref().map_or_else(
            || MOVING.to_vec(),
            |moving| moving.iter().map(String::as_str).collect(),
        );
        texts
            .into_iter()
            .map(|text| Glob::new(text).map_err(|error| format!("moving: {error}")))
            .collect()
    }

    /// Which repositories of the catalog the source takes.
    ///
    /// # Errors
    ///
    /// When `include` or `exclude` holds a glob that is not one.
    pub fn selection(&self) -> Result<Selection, String> {
        let globs = |texts: &[String]| {
            texts
                .iter()
                .map(|text| Glob::new(text).map_err(|error| error.to_string()))
                .collect::<Result<Vec<Glob>, String>>()
        };
        Ok(Selection {
            include: globs(&self.include)?,
            exclude: globs(&self.exclude)?,
            visibility: vec![
                Visibility::Public,
                Visibility::Internal,
                Visibility::Private,
            ],
            ..Selection::default()
        })
    }

    /// The most repositories the catalog reads.
    #[must_use]
    pub fn most_listed(&self) -> NonZeroUsize {
        self.most_listed.unwrap_or(MOST_LISTED)
    }

    /// The most repositories the selection keeps.
    #[must_use]
    pub fn most_members(&self) -> NonZeroUsize {
        self.most_members.unwrap_or(MOST_MEMBERS)
    }

    /// How long a repository that left the catalog is kept.
    #[must_use]
    pub fn keep_members(&self) -> SignedDuration {
        self.keep_members.map_or(KEEP_MEMBERS, |kept| kept.0)
    }

    /// How many entries one page of a listing asks for.
    #[must_use]
    pub fn listed_page(&self) -> usize {
        self.listed_page.map_or(LISTED_PAGE, NonZeroUsize::get)
    }

    /// The most tags a repository's listing reads.
    #[must_use]
    pub fn most_tags(&self) -> NonZeroUsize {
        self.most_tags.unwrap_or(MOST_TAGS)
    }

    /// Who may see the repositories: anyone where the source reads
    /// anonymously or declares them public, and those given access where
    /// its token may read private ones.
    #[must_use]
    pub fn visibility(&self) -> Visibility {
        if self.token.is_none() || self.visibility == Some(Declared::Public) {
            Visibility::Public
        } else {
            Visibility::Private
        }
    }

    /// How a poll reads: the cursor remembering `most_tags` identities
    /// where `remembered` is absent.
    #[must_use]
    pub fn reading(&self) -> Reading {
        Reading::of(
            self.page,
            self.remembered.or(Some(self.most_tags())),
            self.parse,
            herald_wire::PARSE_LIMIT,
        )
    }

    /// The key that bounds how many identities the cursor remembers.
    #[must_use]
    pub const fn remembering_key(&self) -> &'static str {
        match self.remembered {
            Some(_) => "remembered",
            None => "most_tags",
        }
    }

    /// Whether the options are sound.
    ///
    /// # Errors
    ///
    /// Why they are not, naming the key to change.
    pub fn check(&self) -> Result<(), (&'static str, String)> {
        let address = Address::parse(&self.registry).filter(|_| web_url(&self.registry));
        match address {
            None => {
                return Err((
                    "registry",
                    format!(
                        "\"{}\" is not a web address; give the registry's base URL, such as https://registry.example",
                        self.registry
                    ),
                ));
            }
            Some(address) if address.names_a_user() => {
                return Err((
                    "registry",
                    "the registry's URL names a user; give username and token instead".to_owned(),
                ));
            }
            Some(_) => {}
        }
        self.repository_check()?;
        self.credentials_check()?;
        self.origins_check()?;
        self.registry_name_check()?;
        self.moving().map_err(|error| ("moving", error))?;
        self.tags_check()
    }

    /// Whether one of `repository` and `catalog` is given, the repository
    /// written as a registry names one, and the catalog's keys only beside
    /// `catalog`.
    fn repository_check(&self) -> Result<(), (&'static str, String)> {
        match (&self.repository, self.catalog) {
            (Some(_), true) => Err((
                "catalog",
                "repository and catalog name different things; give one of them".to_owned(),
            )),
            (None, false) => Err((
                "repository",
                "give repository, or catalog = true to take every repository the registry lists"
                    .to_owned(),
            )),
            (Some(repository), false) => {
                if !names_repository(repository) {
                    return Err((
                        "repository",
                        format!(
                            "\"{repository}\" is not a repository; give it as names of letters, digits, '.', '_' and '-' joined by '/', such as acme/app"
                        ),
                    ));
                }
                let given = [
                    !self.include.is_empty(),
                    !self.exclude.is_empty(),
                    self.most_listed.is_some(),
                    self.most_members.is_some(),
                    self.keep_members.is_some(),
                ];
                CATALOGUED
                    .into_iter()
                    .zip(given)
                    .find_map(|(key, given)| given.then_some(key))
                    .map_or(Ok(()), |key| {
                        Err((
                            key,
                            format!(
                                "{key} chooses among the catalog's repositories; give catalog = true instead of repository, or remove {key}"
                            ),
                        ))
                    })
            }
            (None, true) => {
                self.selection().map_err(|error| ("include", error))?;
                if self.most_members().get() > MOST_MEMBERS_ALLOWED {
                    return Err((
                        "most_members",
                        format!("most_members is at most {MOST_MEMBERS_ALLOWED}; lower it"),
                    ));
                }
                Ok(())
            }
        }
    }

    /// Whether `username` and `token` are given together.
    fn credentials_check(&self) -> Result<(), (&'static str, String)> {
        match (&self.username, &self.token) {
            (Some(_), None) => Err((
                "username",
                "a username is sent with its token; give token, or remove username".to_owned(),
            )),
            (None, Some(_)) => Err((
                "token",
                "a token is sent with the account it belongs to; give username".to_owned(),
            )),
            _ => Ok(()),
        }
    }

    /// Whether `auth` and each of `blobs` is a web address.
    fn origins_check(&self) -> Result<(), (&'static str, String)> {
        if let Some(auth) = self.auth.as_ref().filter(|auth| origin_of(auth).is_none()) {
            return Err((
                "auth",
                format!(
                    "\"{auth}\" is not a web address naming no user; give the base URL of the registry's token service, such as https://auth.docker.io"
                ),
            ));
        }
        match self
            .blobs
            .iter()
            .flatten()
            .find(|blob| origin_of(blob).is_none())
        {
            Some(blob) => Err((
                "blobs",
                format!(
                    "\"{blob}\" is not a web address naming no user; give the base URL a blob is redirected to, such as https://cdn.registry.example"
                ),
            )),
            None => Ok(()),
        }
    }

    /// Whether `registry_name` is a host, with a port where it names one.
    fn registry_name_check(&self) -> Result<(), (&'static str, String)> {
        match &self.registry_name {
            Some(name)
                if Address::parse(&format!("https://{name}"))
                    .is_none_or(|address| address.authority() != *name) =>
            {
                Err((
                    "registry_name",
                    format!(
                        "\"{name}\" is not a registry's host; give it in lowercase, as docker.io or registry.example:5000"
                    ),
                ))
            }
            _ => Ok(()),
        }
    }

    /// Whether the listing's page, the tags it reads and the cursor that
    /// remembers them are sound.
    fn tags_check(&self) -> Result<(), (&'static str, String)> {
        if self.listed_page() > LISTED_PAGE_ALLOWED {
            return Err((
                "listed_page",
                format!("listed_page is at most {LISTED_PAGE_ALLOWED}; lower it"),
            ));
        }
        let most_tags = self.most_tags().get();
        if most_tags > MOST_TAGS_ALLOWED {
            return Err((
                "most_tags",
                format!("most_tags is at most {MOST_TAGS_ALLOWED}; lower it"),
            ));
        }
        let reading = self.reading();
        if reading.remembered < most_tags {
            return Err((
                "remembered",
                format!(
                    "the cursor remembers every tag a repository lists, so remembered is at least most_tags, {most_tags}; raise it, or lower most_tags"
                ),
            ));
        }
        let key = if reading.remembered < reading.page {
            "page"
        } else {
            "parse"
        };
        reading
            .check(herald_wire::PARSE_LIMIT)
            .map_err(|error| (key, error))
    }
}

/// Whether `repository` is written as a registry names a repository: names
/// of letters, digits, '.', '_' and '-', joined by '/'.
pub(crate) fn names_repository(repository: &str) -> bool {
    path_of_names(repository, 1..=usize::MAX)
}
