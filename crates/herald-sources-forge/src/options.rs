use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;

use herald_core::{
    Address, Duration, Glob, Prefer, PrereleaseRule, ProjectName, REPOSITORY_FIELDS, Sandbox,
    SecretRef, Selection, Signal, Size, Visibility, path_of_names, web_url,
};
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

/// Where an owner source's member file is.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum Members {
    /// A file beside the config, such as `repos.toml`.
    File {
        /// Its path, from the config's directory.
        file: String,
    },
    /// A file in a repository on the source's forge, read through its API
    /// with the source's token each round.
    Repository {
        /// The repository, as `repository` writes one.
        repository: String,
        /// The file's path in it.
        path: String,
        /// The branch, tag or commit it is read at; the default branch where
        /// absent.
        #[serde(rename = "ref")]
        reference: Option<String>,
    },
}

/// What an owner source makes of the repositories one glob names.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Override {
    /// The project their events are announced as, a template as the
    /// source's `project` is; the source's where absent.
    pub project: Option<String>,
    /// What they announce, as the source's `signals` writes it; the
    /// source's where absent.
    pub signals: Option<Vec<Signal>>,
    /// How a pre-release of theirs is told, as the source's `prerelease`
    /// writes it; the source's where absent.
    pub prerelease: Option<PrereleaseRule>,
}

/// What a source announces of one repository, and how it tells a
/// pre-release there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signals {
    /// The signals it announces.
    pub taken: BTreeSet<Signal>,
    /// How it tells a pre-release.
    pub prerelease: PrereleaseRule,
}

impl Default for Signals {
    fn default() -> Self {
        Self {
            taken: Signal::DEFAULT.into(),
            prerelease: PrereleaseRule::default(),
        }
    }
}

impl Signals {
    /// Whether it announces `signal`.
    #[must_use]
    pub fn takes(&self, signal: Signal) -> bool {
        self.taken.contains(&signal)
    }
}

/// The field each entry of a member file names its repository by where
/// `key` is absent.
pub const KEY: &str = "name";

/// How many of an owner's members one GitHub GraphQL request asks for
/// where `graphql_batch` is absent.
pub const GRAPHQL_BATCH: NonZeroUsize = NonZeroUsize::new(50).unwrap();
/// The most `graphql_batch` may be: the most ids GitHub's GraphQL API
/// looks up in one request.
pub const GRAPHQL_BATCH_ALLOWED: usize = 100;

/// How an owner's members are polled on GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// GraphQL in batches where the source has a token, and REST per member
    /// where it has none.
    #[default]
    Auto,
    /// GraphQL in batches of `graphql_batch` members, which needs a token.
    Graphql,
    /// REST, one member at a time, each asked with its `ETag`.
    Rest,
    /// The owner's events, read once a round, naming the members to poll,
    /// each through GraphQL where there is a token and REST otherwise.
    Events,
}

/// A forge source's options in `herald.toml` (ADR-0006, ADR-0029, ADR-0030).
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
    /// How many of the newest releases and tags the first poll announces;
    /// none where absent (ADR-0003). For an owner, it applies to the members
    /// the first listing finds; a member found later announces only what it
    /// publishes after it was found.
    #[serde(default)]
    pub backfill: usize,
    /// The secret the forge signs its webhook deliveries to `herald server`
    /// with: GitHub's and Forgejo's webhook secret, or GitLab's signing
    /// token (ADR-0007). An owner source takes its repositories' and its
    /// organization's deliveries, and announces one only where the owner's
    /// last membership record holds the repository when its work runs.
    pub webhook: Option<SecretRef>,
    /// How many of the newest releases, and of the newest tags, one poll
    /// reads; 30 where absent.
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
    /// repository whose visibility the forge does not name is private. A
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
    /// The member file: where given, the names it lists are the source's
    /// members, which the filters narrow, read as TOML or JSON by its
    /// extension.
    pub members: Option<Members>,
    /// The arrays of the member file that list repositories, such as
    /// `["repo", "ungoverned"]`; the file's own array where absent, which
    /// only JSON can hold.
    #[serde(default)]
    pub tables: Vec<String>,
    /// The field each entry of the member file names its repository by;
    /// "name" where absent.
    pub key: Option<String>,
    /// The project a member's events are announced as, a template of
    /// `repository.name`, `repository.path`, `repository.owner`,
    /// `repository.id`, `repository.topics`, `repository.properties` and
    /// `repository.visibility`; the repository's name where absent.
    #[schemars(example = SourceOptions::PROJECT)]
    pub project: Option<String>,
    /// What the source makes of the repositories each glob names, as
    /// `include` writes them; where several name one repository, the
    /// narrowest that sets a key gives it.
    #[serde(default)]
    pub repositories: BTreeMap<String, Override>,
    /// How a GitHub owner's members are polled; "auto" where absent.
    pub transport: Option<Transport>,
    /// The GraphQL endpoint of a GitHub owner source; beside `api` where
    /// absent: `/api/graphql` on a GitHub Enterprise Server whose `api` ends
    /// in `/api/v3`, and `/graphql` on GitHub and GHE.com.
    pub graphql: Option<String>,
    /// How many members one GraphQL request asks for; 50 where absent, and
    /// at most 100.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub graphql_batch: Option<NonZeroUsize>,
    /// What the source announces of each repository: its published Releases,
    /// its version tags, or both; `["release"]` where absent. A tag and a
    /// Release of it share one identity, so each route announces it once.
    #[schemars(example = SourceOptions::SIGNALS)]
    pub signals: Option<Vec<Signal>>,
    /// How a pre-release is told: by the forge's flag or a pre-release part
    /// in the version ("either"), by the flag alone ("flag"), or by the
    /// version alone ("tag"); "either" where absent. A tag carries no flag,
    /// and neither does a GitLab release.
    pub prerelease: Option<PrereleaseRule>,
    /// Which of a tag and its Release is announced where a repository's
    /// signals take both: the Release, a tag with none waiting for it as
    /// long as `tag_window` ("release"), or whichever comes first
    /// ("first"); "release" where absent. Holding a tag needs a store that
    /// keeps held events, which the markers store does not.
    pub prefer: Option<Prefer>,
    /// How long a tag with no Release waits for one, counted from when
    /// herald first sees the tag; "1h" where absent, and at most "7d".
    pub tag_window: Option<Duration>,
}

/// How long a tag waits for its Release where `tag_window` is absent.
pub const TAG_WINDOW: SignedDuration = SignedDuration::from_hours(1);
/// The longest `tag_window` may be.
pub const TAG_WINDOW_ALLOWED: SignedDuration = SignedDuration::from_hours(7 * 24);

/// The keys that only an owner source may give.
const OWNED: [&str; 22] = [
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
    "members",
    "tables",
    "key",
    "project",
    "repositories",
    "transport",
    "graphql",
    "graphql_batch",
];

/// Whether an override sets one of its keys.
type Sets = fn(&Override) -> bool;

const fn sets_project(chosen: &Override) -> bool {
    chosen.project.is_some()
}

const fn sets_signals(chosen: &Override) -> bool {
    chosen.signals.is_some()
}

const fn sets_prerelease(chosen: &Override) -> bool {
    chosen.prerelease.is_some()
}

/// Each key of an override that the narrowest glob naming a repository
/// gives it, and whether an override sets it.
const OVERRIDDEN: [(&str, Sets); 3] = [
    ("project", sets_project),
    ("signals", sets_signals),
    ("prerelease", sets_prerelease),
];

/// Two of `globs` as narrow as each other that could both name one
/// repository, where two are.
fn tie(globs: &[Glob]) -> Option<(&Glob, &Glob)> {
    globs.iter().enumerate().find_map(|(at, glob)| {
        globs
            .iter()
            .skip(at + 1)
            .find(|other| other.specificity() == glob.specificity() && other.overlaps(glob))
            .map(|other| (glob, other))
    })
}

/// The project template `source`, compiled, where it compiles.
fn compiled(source: &str) -> Option<ProjectName> {
    ProjectName::compile(source, Sandbox::DEFAULT).ok()
}

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
    /// The project template the schema shows as an example.
    const PROJECT: &str = "{{ repository.owner }}/{{ repository.name }}";
    /// The signals the schema shows as an example.
    const SIGNALS: [Signal; 2] = [Signal::Release, Signal::Tag];

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
        self.signals_check(forge)?;
        self.prefer_check()?;
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
        if self.subgroups.is_some() && forge != Forge::GitLab {
            return Err("subgroups is a GitLab group's; remove it".to_owned());
        }
        if forge != Forge::GitHub
            && let Some(key) = [
                (self.transport.is_some(), "transport"),
                (self.graphql.is_some(), "graphql"),
                (self.graphql_batch.is_some(), "graphql_batch"),
            ]
            .into_iter()
            .find_map(|(given, key)| given.then_some(key))
        {
            return Err(format!("{key} is a GitHub owner's; remove it"));
        }
        if self.transport == Some(Transport::Graphql) && self.token.is_none() {
            return Err(
                "transport graphql needs a token, as GitHub's GraphQL API answers none without; give token, or choose rest".to_owned(),
            );
        }
        if let Some(graphql) = &self.graphql {
            if !web_url(graphql) {
                return Err(format!(
                    "graphql \"{graphql}\" is not a web address; give the GraphQL endpoint, such as https://ghe.example/api/graphql"
                ));
            }
            let origin = |url: &str| Address::parse(url).map(|address| address.origin());
            if origin(graphql) != origin(self.api.as_deref().unwrap_or(forge.api().url)) {
                return Err(format!(
                    "graphql \"{graphql}\" is not on api's host, the only one the source may reach; give the endpoint beside api, or remove graphql"
                ));
            }
        }
        if self.graphql_batch().get() > GRAPHQL_BATCH_ALLOWED {
            return Err(format!(
                "graphql_batch is at most {GRAPHQL_BATCH_ALLOWED}; lower it"
            ));
        }
        if self.members.is_none() && (!self.tables.is_empty() || self.key.is_some()) {
            return Err(
                "tables and key read a member file; give members, or remove them".to_owned(),
            );
        }
        if let Some(Members::Repository {
            repository, path, ..
        }) = &self.members
        {
            if !path_of_names(repository, forge.names()) {
                return Err(format!(
                    "members.repository \"{repository}\" is not a repository; give it as {}",
                    forge.shape()
                ));
            }
            if path
                .split('/')
                .any(|segment| matches!(segment, "" | "." | ".."))
            {
                return Err(format!(
                    "members.path \"{path}\" is not a file's path in the repository; give it as dir/file.toml, without empty, . or .. parts"
                ));
            }
        }
        if let Some(project) = &self.project {
            Self::project_check(project).map_err(|reason| format!("project {reason}"))?;
        }
        for (glob, chosen) in &self.repositories {
            Glob::new(glob).map_err(|error| format!("repositories: {error}"))?;
            if let Some(project) = &chosen.project {
                Self::project_check(project)
                    .map_err(|reason| format!("repositories.\"{glob}\".project {reason}"))?;
            }
        }
        for (key, sets) in OVERRIDDEN {
            let globs: Vec<Glob> = self
                .repositories
                .iter()
                .filter(|(_, chosen)| sets(chosen))
                .filter_map(|(glob, _)| Glob::new(glob).ok())
                .collect();
            if let Some((glob, other)) = tie(&globs) {
                return Err(format!(
                    "repositories \"{glob}\" and \"{other}\" both set {key}, are as narrow as each other and could both name one repository; make one narrower"
                ));
            }
        }
        if self.most_members().get() > MOST_MEMBERS_ALLOWED {
            return Err(format!(
                "most_members is at most {MOST_MEMBERS_ALLOWED}; lower it"
            ));
        }
        self.selection().map(|_| ())
    }

    /// Whether the signals and pre-release rules, the source's and each
    /// override's, are sound on `forge`.
    fn signals_check(&self, forge: Forge) -> Result<(), String> {
        let overrides = self.repositories.iter().map(|(glob, chosen)| {
            (
                format!("repositories.\"{glob}\"."),
                &chosen.signals,
                chosen.prerelease,
            )
        });
        for (at, signals, prerelease) in
            std::iter::once((String::new(), &self.signals, self.prerelease)).chain(overrides)
        {
            if signals.as_ref().is_some_and(Vec::is_empty) {
                return Err(format!(
                    "{at}signals names no signal; give release, tag or both, or remove it"
                ));
            }
            if forge == Forge::GitLab && prerelease == Some(PrereleaseRule::Flag) {
                return Err(format!(
                    "{at}prerelease \"flag\" marks nothing on GitLab, whose releases carry no pre-release flag; choose either or tag"
                ));
            }
        }
        Ok(())
    }

    /// Whether `prefer` and `tag_window` choose between a tag and its
    /// Release that some repository the source takes announces both of.
    fn prefer_check(&self) -> Result<(), String> {
        let both = self.may_take(Signal::Release) && self.may_take(Signal::Tag);
        if let Some(key) = [
            (self.prefer.is_some(), "prefer"),
            (self.tag_window.is_some(), "tag_window"),
        ]
        .into_iter()
        .find_map(|(given, key)| (given && !both).then_some(key))
        {
            return Err(format!(
                "{key} chooses between a tag and its Release, which no repository the source takes announces both of; give signals = [\"release\", \"tag\"], or remove {key}"
            ));
        }
        if self.tag_window() > TAG_WINDOW_ALLOWED {
            return Err("tag_window is at most \"7d\"; shorten it".to_owned());
        }
        if self.tag_window.is_some() && self.prefer() == Prefer::First {
            return Err(
                "tag_window is how long a tag waits for its Release, which prefer = \"first\" never does; remove one".to_owned(),
            );
        }
        Ok(())
    }

    /// Which of a tag and its Release is announced.
    #[must_use]
    pub fn prefer(&self) -> Prefer {
        self.prefer.unwrap_or_default()
    }

    /// How long a tag with no Release waits for one.
    #[must_use]
    pub fn tag_window(&self) -> SignedDuration {
        self.tag_window.map_or(TAG_WINDOW, |window| window.0)
    }

    /// How long a tag of a repository whose signals are `signals` waits for
    /// its Release, where it waits: where they take both and the source
    /// prefers the Release.
    #[must_use]
    pub fn held_for(&self, signals: &Signals) -> Option<SignedDuration> {
        (self.prefer() == Prefer::Release
            && signals.takes(Signal::Release)
            && signals.takes(Signal::Tag))
        .then(|| self.tag_window())
    }

    /// What the source announces of the repository named `name`, and how it
    /// tells a pre-release there: for each, the narrowest of `repositories`
    /// naming it that sets it, or else the source's.
    #[must_use]
    pub fn signals_of(&self, name: &str) -> Signals {
        let narrowest = |sets: Sets| {
            self.repositories
                .iter()
                .filter(|(_, chosen)| sets(chosen))
                .filter_map(|(glob, chosen)| Some((Glob::new(glob).ok()?, chosen)))
                .filter(|(glob, _)| glob.matches(name))
                .max_by_key(|(glob, _)| glob.specificity())
                .map(|(_, chosen)| chosen)
        };
        let signals = narrowest(sets_signals)
            .and_then(|chosen| chosen.signals.as_ref())
            .or(self.signals.as_ref());
        let prerelease = narrowest(sets_prerelease)
            .and_then(|chosen| chosen.prerelease)
            .or(self.prerelease);
        let defaults = Signals::default();
        Signals {
            taken: signals.map_or(defaults.taken, |signals| signals.iter().copied().collect()),
            prerelease: prerelease.unwrap_or(defaults.prerelease),
        }
    }

    /// Whether any repository the source takes may announce `signal`.
    #[must_use]
    pub fn may_take(&self, signal: Signal) -> bool {
        let taken = |signals: &Option<Vec<Signal>>| {
            signals.as_ref().map_or_else(
                || Signals::default().takes(signal),
                |signals| signals.contains(&signal),
            )
        };
        taken(&self.signals)
            || self
                .repositories
                .values()
                .any(|chosen| sets_signals(chosen) && taken(&chosen.signals))
    }

    /// Whether the `project` template compiles and reads a repository's
    /// attributes alone.
    fn project_check(project: &str) -> Result<(), String> {
        let named =
            ProjectName::compile(project, Sandbox::DEFAULT).map_err(|error| error.reason)?;
        let unknown = named.unknown();
        if unknown.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "reads {}, which a repository does not carry; use repository.{}",
                unknown.join(", "),
                REPOSITORY_FIELDS.join(", repository.")
            ))
        }
    }

    /// The project template, compiled, where one is given.
    #[must_use]
    pub fn project(&self) -> Option<ProjectName> {
        self.project.as_deref().and_then(compiled)
    }

    /// Each glob of `repositories` that sets a project, with its template
    /// compiled, narrowest first.
    #[must_use]
    pub fn projects(&self) -> Vec<(Glob, ProjectName)> {
        let mut projects: Vec<(Glob, ProjectName)> = self
            .repositories
            .iter()
            .filter_map(|(glob, chosen)| {
                Some((
                    Glob::new(glob).ok()?,
                    chosen.project.as_deref().and_then(compiled)?,
                ))
            })
            .collect();
        projects.sort_by_key(|(glob, _)| std::cmp::Reverse(glob.specificity()));
        projects
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
            self.members.is_some(),
            !self.tables.is_empty(),
            self.key.is_some(),
            self.project.is_some(),
            !self.repositories.is_empty(),
            self.transport.is_some(),
            self.graphql.is_some(),
            self.graphql_batch.is_some(),
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
            names: None,
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

    /// How many members one GraphQL request asks for.
    #[must_use]
    pub fn graphql_batch(&self) -> NonZeroUsize {
        self.graphql_batch.unwrap_or(GRAPHQL_BATCH)
    }

    /// How a GitHub owner's members are polled.
    #[must_use]
    pub fn transport(&self) -> Transport {
        self.transport.unwrap_or_default()
    }

    /// Whether a GitHub owner's members are polled together through
    /// GraphQL: as `transport` says, and under "auto" and "events" where
    /// there is a token.
    #[must_use]
    pub fn batched(&self) -> bool {
        match self.transport() {
            Transport::Graphql => true,
            Transport::Auto | Transport::Events => self.token.is_some(),
            Transport::Rest => false,
        }
    }

    /// The field each entry of the member file names its repository by.
    #[must_use]
    pub fn key(&self) -> &str {
        self.key.as_deref().unwrap_or(KEY)
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
