use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use herald_core::{Duration, Glob, SecretRef, Selection, Size, Visibility, path_of_names, web_url};
use herald_wire::{PARSE_LIMIT, Reading};
use jiff::SignedDuration;

use crate::Forge;

/// The most repositories a listing reads, before the filters, where
/// `most_listed` is absent.
pub const MOST_LISTED: NonZeroUsize = NonZeroUsize::new(10_000).unwrap();
/// The most members a selection keeps where `most_members` is absent.
pub const MOST_MEMBERS: NonZeroUsize = NonZeroUsize::new(1_000).unwrap();
/// The most `most_members` may be.
pub const MOST_MEMBERS_ALLOWED: usize = 100_000;
/// How long a member that left is kept where `keep_members` is absent.
pub const KEEP_MEMBERS: SignedDuration = SignedDuration::from_hours(30 * 24);

/// A property's values: one, or any of several.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Values {
    /// One value.
    One(String),
    /// Any one of several values.
    Any(Vec<String>),
}

impl Values {
    fn listed(&self) -> Vec<String> {
        match self {
            Self::One(value) => vec![value.clone()],
            Self::Any(values) => values.clone(),
        }
    }
}

/// A forge source's options in `herald.toml` (ADR-0006, ADR-0029).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceOptions {
    /// The repository, as `owner/name`, or a GitLab project's full path,
    /// such as `group/subgroup/name`; or `owner` in its place.
    #[schemars(example = "nightworksio/herald")]
    pub repository: Option<String>,
    /// The organization, GitLab group or user whose repositories are the
    /// source's members, each polled on its own, in place of `repository`.
    #[schemars(example = SourceOptions::OWNER)]
    pub owner: Option<String>,
    /// The API's base URL, for a forge that is not GitHub, GitLab.com or
    /// Codeberg: a GitHub Enterprise Server's `https://ghe.example/api/v3`,
    /// a GHE.com tenant's `https://api.acme.ghe.com`, a self-managed or
    /// Dedicated GitLab's `https://gitlab.example/api/v4`, or a Forgejo's
    /// `https://forge.example/api/v1`. Where absent, it is the CI run's own
    /// when announcing the run's repository, and otherwise the forge's
    /// public instance. An owner is listed at the same base.
    pub api: Option<String>,
    /// A token that reaches a private repository or raises the API's rate
    /// limit, such as the run's `GITHUB_TOKEN`.
    pub token: Option<SecretRef>,
    /// How many of the newest releases the first poll announces; none where
    /// absent (ADR-0003). For an owner, it applies to the members the first
    /// listing finds; a member found later announces only what it publishes
    /// after it was found.
    #[serde(default)]
    pub backfill: usize,
    /// The secret the forge signs its webhook deliveries to `herald server`
    /// with: GitHub's and Forgejo's webhook secret, or GitLab's signing
    /// token (ADR-0007). A source of one repository only.
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
    /// The names of the owner's repositories taken, as globs in which `*`
    /// stands for any run of characters, `?` for any one and `[...]` for
    /// any one of a class, ignoring case; every name where absent.
    #[serde(default)]
    #[schemars(example = SourceOptions::INCLUDE)]
    pub include: Vec<String>,
    /// The names left out, as `include` writes them, whatever `include`
    /// says.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Topics, any one of which a member holds; any where absent.
    #[serde(default)]
    pub topics: Vec<String>,
    /// The visibilities taken; only public repositories where absent. A
    /// route that takes a private or internal member is refused unless it
    /// is `private` (`[doctor] private_to_public`).
    pub visibility: Option<Vec<Visibility>>,
    /// Whether archived repositories are taken.
    #[serde(default)]
    pub archived: bool,
    /// Whether forks are taken.
    #[serde(default)]
    pub fork: bool,
    /// Whether template repositories are taken.
    #[serde(default)]
    pub template: bool,
    /// Languages, any one of which a member is written in; any where
    /// absent.
    #[serde(default)]
    pub language: Vec<String>,
    /// Custom properties, every one of which a member holds with one of the
    /// values given, such as `{ team = "media", tier = ["core", "edge"] }`.
    #[serde(default)]
    pub properties: BTreeMap<String, Values>,
    /// Whether a GitLab group's members include the projects of its
    /// subgroups; true where absent.
    pub subgroups: Option<bool>,
    /// The most repositories a listing reads before the filters; 10000
    /// where absent. A listing over it refuses the source for the round.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub most_listed: Option<NonZeroUsize>,
    /// The most members the filters may keep; 1000 where absent, and at
    /// most 100000. A selection over it refuses the source for the round.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub most_members: Option<NonZeroUsize>,
    /// How long a member that left the selection keeps its cursor before it
    /// is forgotten; "30d" where absent.
    pub keep_members: Option<Duration>,
    /// How many repositories one page of the listing asks for; 100 where
    /// absent on GitHub and GitLab, and 50 on Forgejo, whose instances
    /// answer at most 50 unless configured otherwise.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub listed_page: Option<NonZeroUsize>,
}

/// The keys that only an owner source may give.
const OWNED: [&str; 14] = [
    "include",
    "exclude",
    "topics",
    "visibility",
    "archived",
    "fork",
    "template",
    "language",
    "properties",
    "subgroups",
    "most_listed",
    "most_members",
    "keep_members",
    "listed_page",
];

fn globs(texts: &[String]) -> Result<Vec<Glob>, String> {
    texts
        .iter()
        .map(|text| Glob::new(text).map_err(|error| error.to_string()))
        .collect()
}

impl SourceOptions {
    /// The names the schema shows `include` taking as an example.
    const INCLUDE: [&str; 1] = ["plugin-*"];
    /// The owner the schema shows as an example.
    const OWNER: &str = "nightworksio";

    /// Whether the options are sound for a source on `forge`.
    ///
    /// # Errors
    ///
    /// When neither or both of `repository` and `owner` are given, either
    /// is not a path the forge names one by, an owner's key is given
    /// without `owner`, a glob cannot be read, `most_members` is over its
    /// most, or `api` is not a web address.
    pub fn check(&self, forge: Forge) -> Result<(), String> {
        match (&self.repository, &self.owner) {
            (Some(repository), None) => self.repository_check(forge, repository)?,
            (None, Some(owner)) => self.owner_check(forge, owner)?,
            (Some(_), Some(_)) => {
                return Err("a source names either a repository or an owner; remove one".to_owned());
            }
            (None, None) => {
                return Err(format!(
                    "a source names a repository, as {}, or an owner",
                    forge.shape()
                ));
            }
        }
        match &self.api {
            Some(api) if !web_url(api) => Err(format!(
                "\"{api}\" is not a web address; give the API's base URL, such as https://forge.example/api/v1"
            )),
            _ => self.reading().check(PARSE_LIMIT),
        }
    }

    fn repository_check(&self, forge: Forge, repository: &str) -> Result<(), String> {
        if !path_of_names(repository, forge.names()) {
            return Err(format!(
                "\"{repository}\" is not a repository; give it as {}, of letters, digits, '.', '_' and '-'",
                forge.shape()
            ));
        }
        match self.owned().first() {
            Some(key) => Err(format!(
                "{key} chooses among an owner's repositories; give owner in place of repository, or remove {key}"
            )),
            None => Ok(()),
        }
    }

    fn owner_check(&self, forge: Forge, owner: &str) -> Result<(), String> {
        if !path_of_names(owner, forge.owners()) {
            return Err(format!(
                "\"{owner}\" is not an owner; give it as {}, of letters, digits, '.', '_' and '-'",
                forge.owner_shape()
            ));
        }
        if self.webhook.is_some() {
            return Err(
                "an owner source is polled, and takes no webhook; remove webhook".to_owned(),
            );
        }
        if self.subgroups.is_some() && forge != Forge::GitLab {
            return Err("subgroups is a GitLab group's; remove it".to_owned());
        }
        if self.most_members().get() > MOST_MEMBERS_ALLOWED {
            return Err(format!(
                "most_members is at most {MOST_MEMBERS_ALLOWED}; lower it"
            ));
        }
        self.selection().map(|_| ())
    }

    /// Each owner's key the options give.
    fn owned(&self) -> Vec<&'static str> {
        let given = [
            !self.include.is_empty(),
            !self.exclude.is_empty(),
            !self.topics.is_empty(),
            self.visibility.is_some(),
            self.archived,
            self.fork,
            self.template,
            !self.language.is_empty(),
            !self.properties.is_empty(),
            self.subgroups.is_some(),
            self.most_listed.is_some(),
            self.most_members.is_some(),
            self.keep_members.is_some(),
            self.listed_page.is_some(),
        ];
        OWNED
            .into_iter()
            .zip(given)
            .filter_map(|(key, given)| given.then_some(key))
            .collect()
    }

    /// Which of the owner's repositories the source takes.
    ///
    /// # Errors
    ///
    /// When a glob of `include` or `exclude` cannot be read.
    pub fn selection(&self) -> Result<Selection, String> {
        Ok(Selection {
            include: globs(&self.include)?,
            exclude: globs(&self.exclude)?,
            topics: self.topics.clone(),
            visibility: self
                .visibility
                .clone()
                .unwrap_or_else(|| Selection::default().visibility),
            archived: self.archived,
            fork: self.fork,
            template: self.template,
            language: self.language.clone(),
            properties: self
                .properties
                .iter()
                .map(|(name, values)| (name.clone(), values.listed()))
                .collect(),
        })
    }

    /// The most repositories a listing reads before the filters.
    #[must_use]
    pub fn most_listed(&self) -> NonZeroUsize {
        self.most_listed.unwrap_or(MOST_LISTED)
    }

    /// The most members the filters may keep.
    #[must_use]
    pub fn most_members(&self) -> NonZeroUsize {
        self.most_members.unwrap_or(MOST_MEMBERS)
    }

    /// How long a member that left is kept.
    #[must_use]
    pub fn keep_members(&self) -> SignedDuration {
        self.keep_members.map_or(KEEP_MEMBERS, |kept| kept.0)
    }

    /// Whether a GitLab group's members include its subgroups' projects.
    #[must_use]
    pub fn subgroups(&self) -> bool {
        self.subgroups.unwrap_or(true)
    }

    /// How a poll of the source reads.
    #[must_use]
    pub fn reading(&self) -> Reading {
        Reading::of(self.page, self.remembered, self.parse, PARSE_LIMIT)
    }
}

#[cfg(test)]
mod tests;
