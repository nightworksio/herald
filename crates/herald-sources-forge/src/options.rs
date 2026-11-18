use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use herald_core::{
    Duration, Glob, Prefer, PrereleaseRule, SecretRef, Signal, Size, Visibility, path_of_names,
    web_url,
};
use herald_wire::{PARSE_LIMIT, Reading};

use crate::Forge;

pub(crate) use alerts::AlertChoice;
pub use alerts::{ALERT_ACTIONS, AlertAction, Alerts, Severity, Tool};
pub use discovery::Discovery;
pub use hooks::MemberHooks;
pub(crate) use issues::IssueChoice;
pub use issues::{ISSUE_ACTIONS, IssueAction, Issues};
pub use owner::{
    KEEP_MEMBERS, KEY, MOST_LISTED, MOST_MEMBERS, MOST_MEMBERS_ALLOWED, Members, Values,
};
pub use packages::PackageVisibility;
pub(crate) use packages::{
    GITHUB_PACKAGE_TYPES, MOST_PACKAGES, MOST_PACKAGES_ALLOWED, MOVING, registry_host,
};
pub(crate) use pulls::PullChoice;
pub use pulls::{PULL_ACTIONS, PullAction, Pulls};
pub(crate) use runs::RunChoice;
pub use runs::{Conclusion, RUN_ACTIONS, RUN_FROM, RunAction, RunFrom, Runs};
pub use signals::{EDITS_DEPTH, Signals, TAG_WINDOW};
pub use transport::{FEED_PAGE, FEED_PAGES, GRAPHQL_BATCH, GRAPHQL_BATCH_ALLOWED, Transport};

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
    /// The routes their events also reach, by id, whatever sources those
    /// routes name; each route's other keys still narrow what it takes.
    pub routes: Option<Vec<String>>,
    /// Whether `routes` are the only routes their events reach, leaving out
    /// the routes that name the source; false where absent.
    pub routes_only: Option<bool>,
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
    /// The GitHub App the source reads as, by the name of its
    /// `[github_apps.<name>]` table, in place of `token`: herald mints an
    /// installation token for each round.
    pub app: Option<String>,
    /// The id of the App's installation tokens are minted for; looked up
    /// on the owner, or on the repository, where absent.
    #[schemars(with = "Option<std::num::NonZeroU64>")]
    pub installation: Option<std::num::NonZeroU64>,
    /// How many of the newest releases, tags and package versions the first
    /// poll announces;
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
    /// How an owner source finds the repositories that join or leave it:
    /// listing the owner each round ("list"), or only where it holds no
    /// listing, its selection changed or a delivery said the owner's
    /// repositories changed ("push"); "list" where absent.
    pub discovery: Option<Discovery>,
    /// Who registers a project webhook on each member of a GitLab owner,
    /// whose group cannot send its own on GitLab Free: no one ("off"),
    /// `herald hooks <name>` ("setup"), or each poll round too ("round");
    /// "off" where absent. Each hook points at `[server] url` and is signed
    /// with `webhook`.
    pub member_hooks: Option<MemberHooks>,
    /// How an owner's members are polled; "auto" where absent. GitHub
    /// offers auto, graphql, rest and events; GitLab auto, graphql and rest;
    /// Forgejo auto, rest and feed.
    pub transport: Option<Transport>,
    /// The GraphQL endpoint of a GitHub or GitLab owner source; beside
    /// `api` where absent: `/api/graphql` on a GitHub Enterprise Server
    /// whose `api` ends in `/api/v3` and on GitLab, whose `api` ends in
    /// `/api/v4`, and `/graphql` on GitHub and GHE.com.
    pub graphql: Option<String>,
    /// How many members one GraphQL request asks for; 50 where absent, and
    /// at most 100.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub graphql_batch: Option<NonZeroUsize>,
    /// How many activities one page of a Forgejo owner's feed asks for
    /// under `transport = "feed"`; 50 where absent, and at most 200.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub feed_page: Option<NonZeroUsize>,
    /// How many pages of a Forgejo owner's feed a round reads back before
    /// it polls every member; 3 where absent, and at most 12.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub feed_pages: Option<NonZeroUsize>,
    /// What the source announces of each repository: its published Releases,
    /// its version tags, the versions of its packages, or several;
    /// `["release"]` where absent. A tag and a Release of it share one
    /// identity, so each route announces it once.
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
    /// How many of the newest Releases a poll watches for an edit of their
    /// name or notes, where a route taking the source edits them
    /// (`edits = "release"`); 30 where absent, and at most `page`.
    pub edits_depth: Option<usize>,
    /// The types of package announced where the signals take "package", as
    /// the forge names them, such as `["container", "npm"]`; every type
    /// where absent. GitHub's are npm, maven, rubygems, docker, nuget and
    /// container.
    pub package_types: Option<Vec<String>>,
    /// The packages announced, as globs as `include` writes them; every
    /// package where absent.
    #[serde(default)]
    pub packages: Vec<String>,
    /// The container tags that move from one image to the next, which are
    /// never announced, as globs as `include` writes them; `["latest"]`
    /// where absent.
    pub moving: Option<Vec<String>>,
    /// The host of the container registry an image is named on, as in
    /// `registry.example:5000`: where absent, ghcr.io beside GitHub's API,
    /// `containers.<host>` beside a GitHub Enterprise Server's or a GHE.com
    /// tenant's, each image's own on GitLab, and the instance's on Forgejo.
    pub registry: Option<String>,
    /// The most entries one listing of packages reads; 1000 where absent,
    /// and at most 100000. A listing over it refuses the source for the
    /// round.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub most_packages: Option<NonZeroUsize>,
    /// Who may see a package's version: the stricter of the package's own
    /// visibility and its repository's ("stricter"), or the package's own
    /// ("package"); "stricter" where absent. A version not everyone may see
    /// reaches only `private` routes under `[doctor] private_to_public =
    /// "refuse"`.
    pub package_visibility: Option<PackageVisibility>,
    /// Which runs are announced where the signals take "run": what they
    /// are read from, what they did, how they ended, and their branches,
    /// triggers and workflows; every finished run of the forge's own CI
    /// where absent.
    pub runs: Option<Runs>,
    /// Which security alerts are announced where the signals take
    /// "alert": what raised them, what they did and how severe they are;
    /// every tool's raised and reopened alerts where absent.
    pub alerts: Option<Alerts>,
    /// Which pull requests are announced where the signals take
    /// "pull-request": what they did, whether drafts are, and their labels
    /// and authors; every pull request opened, ready for review or merged
    /// where absent.
    pub pulls: Option<Pulls>,
    /// Which issues are announced where the signals take "issue": what
    /// they did, and their labels and authors; every issue opened, closed
    /// or reopened where absent.
    pub issues: Option<Issues>,
}

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

const fn sets_routes(chosen: &Override) -> bool {
    chosen.routes.is_some()
}

const fn sets_routes_only(chosen: &Override) -> bool {
    chosen.routes_only.is_some()
}

/// Each key of an override that the narrowest glob naming a repository
/// gives it, and whether an override sets it.
const OVERRIDDEN: [(&str, Sets); 5] = [
    ("project", sets_project),
    ("signals", sets_signals),
    ("prerelease", sets_prerelease),
    ("routes", sets_routes),
    ("routes_only", sets_routes_only),
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
        self.app_check(forge)?;
        self.hooks_check(forge)?;
        self.signals_check(forge)?;
        self.prefer_check()?;
        self.packages_check(forge)?;
        self.runs_check(forge)?;
        self.alerts_check(forge)?;
        self.pulls_check()?;
        self.issues_check(forge)?;
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

    /// How a poll of the source reads.
    #[must_use]
    pub fn reading(&self) -> Reading {
        Reading::of(self.page, self.remembered, self.parse, PARSE_LIMIT)
    }
}

mod alerts;
mod app;
mod discovery;
mod hooks;
mod issues;
mod overrides;
mod owner;
mod packages;
mod pulls;
mod runs;
mod signals;
#[cfg(test)]
mod tests;
mod transport;
