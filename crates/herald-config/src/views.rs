//! `[views.*]` and `[reports.*]` as the file writes them, and as a config
//! holds them (ADR-0055).

use std::num::NonZeroUsize;

use std::collections::BTreeMap;

use herald_core::{
    Advising, Capacity, Duration, ReportSchedule, Slot, SourceName, Stages, Turn, View,
};
use herald_kinds::Most;
use jiff::SignedDuration;
use jiff::civil::Weekday;
use schemars::JsonSchema;
use serde::Deserialize;
use toml::Spanned;

/// The view a report shows where it names none, over every source that
/// lists its repository's pull requests; `[views.queue]` replaces it.
pub const BUILT_IN_VIEW: &str = "queue";

/// The template herald ships for a report, which a route names as
/// `template = "herald:queue"` and `[templates."herald:queue"]` replaces.
pub const QUEUE_TEMPLATE: &str = "herald:queue";

/// The title of [`QUEUE_TEMPLATE`].
pub(crate) const QUEUE_TITLE: &str = "{{ view.name }}: {{ view.total }} open pull requests{% if view.stale %}, {{ view.stale }} stale{% endif %}";

/// The body of [`QUEUE_TEMPLATE`]: each group's rows, oldest first, then
/// each batch of bumps.
pub(crate) const QUEUE_BODY: &str = "{% for group in view.groups %}**{{ group.name or \"everyone\" }}**\n{% for row in group.rows %}- [{{ row.repository }}#{{ row.number }}]({{ row.url }}) {{ row.title }}: {{ row.reason }}, {{ row.waited }}{% if row.stale %}, stale{% endif %}\n{% endfor %}\n{% endfor %}{% if view.batches %}**bumps**\n{% for batch in view.batches %}- {{ batch.package or \"unread\" }}{% if batch.to %} to {{ batch.to }}{% endif %}: {{ batch.rows | length }} open, {{ batch.green }} green, {{ batch.failing }} failing\n{% endfor %}{% endif %}{% if not view.total %}Nothing is open.\n{% endif %}{% if view.overviews.advisories %}**advisories**: {{ view.overviews.advisories.count }} open{% if view.overviews.advisories.more %}, more not read{% endif %}\n{% for advisory in view.overviews.advisories.open %}- [{{ advisory.repository }}]({{ advisory.url }}) {{ advisory.tool }}{% if advisory.severity %} {{ advisory.severity }}{% endif %}: {{ advisory.summary or \"unnamed\" }}, {{ advisory.state }}, {{ advisory.age }} old\n{% endfor %}{% for unread in view.overviews.advisories.unread %}- {{ unread.repository }} unread, because {{ unread.reason }}\n{% endfor %}{% endif %}{% if view.overviews.withheld %}{{ view.overviews.withheld }}{% endif %}";

/// The template herald ships for a rule's re-run plan, which a route names
/// as `template = "herald:rerun"` and `[templates."herald:rerun"]`
/// replaces (ADR-0065).
pub const RERUN_TEMPLATE: &str = "herald:rerun";

/// The title of [`RERUN_TEMPLATE`].
pub(crate) const RERUN_TITLE: &str = "{{ event.attributes.repository }}#{{ event.attributes.number }}: {{ event.attributes.check }} looks flaky";

/// The body of [`RERUN_TEMPLATE`]: the evidence, then the exact call, or
/// why none can be made.
pub(crate) const RERUN_BODY: &str = "{{ event.attributes.check }} failed then passed on one head {{ event.attributes.flips }} times; it fails on {{ event.attributes.head }}. {% if event.attributes.call %}A re-run of run {{ event.attributes.run_id }} would call {{ event.attributes.call }}.{% else %}No re-run can be planned: {{ event.attributes.reason }}.{% endif %}";

/// The template herald edits a living post to once its route no longer
/// receives the advisories section it showed: fixed, so no config can make
/// it say what was removed (ADR-0067).
pub const RECALL_TEMPLATE: &str = "herald:recalled";

/// The title and body of [`RECALL_TEMPLATE`].
pub(crate) const RECALL_TEXT: &str = herald_core::RECALLED;

/// The template herald ships for a nudge, which a route names as
/// `template = "herald:nudge"` and `[templates."herald:nudge"]` replaces.
pub const NUDGE_TEMPLATE: &str = "herald:nudge";

/// The title of [`NUDGE_TEMPLATE`].
pub(crate) const NUDGE_TITLE: &str = "{{ event.attributes.repository }}#{{ event.attributes.number }} has waited {{ event.attributes.waited }}";

/// The body of [`NUDGE_TEMPLATE`]: whose turn it is, why, and the logins
/// it waits on, which it names without mentioning.
pub(crate) const NUDGE_BODY: &str = "{{ event.title }}: {{ event.attributes.turn }} ({{ event.attributes.reason }}){% if event.attributes.waiting_on %}, waiting on {{ event.attributes.waiting_on }}{% endif %}.";

/// A view: the open pull requests it shows of the sources it reads, and
/// how many it reads of each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewConfig {
    /// What it shows, and how.
    pub view: View,
    /// The sources it reads, each naming one repository or an owner on a
    /// forge.
    pub sources: Vec<SourceName>,
    /// How many it reads of each.
    pub most: Most,
    /// The task that decides each slot its rules leave untold (ADR-0057).
    pub decide: BTreeMap<Slot, String>,
    /// Where it reads its pull requests (ADR-0061).
    pub mode: ViewMode,
    /// How long kept mode waits at least before listing a source's open
    /// pull requests again; every round where `None`.
    pub reconcile: Option<SignedDuration>,
    /// Whether a live read through a REST listing without a token takes
    /// who may see a repository from what the listing states, rather than
    /// taking each of its rows as private (ADR-0062).
    pub stated: bool,
    /// Whether it predicts conflicts: each pull request's changed paths
    /// read, and each row naming those of its repository and branch that
    /// change a path in common (ADR-0063).
    pub conflicts: bool,
    /// Which repositories its capacity overview reads, where it shows one
    /// (ADR-0066).
    pub capacity: Option<Capacity>,
    /// How its advisories overview reads, where it shows one; shown to
    /// private routes alone (ADR-0067).
    pub advisories: Option<Advising>,
}

/// Where a view reads its open pull requests (ADR-0061).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ViewMode {
    /// Kept, of each source that takes webhooks where the store keeps pull
    /// requests; live otherwise.
    #[default]
    Auto,
    /// From the forge, each time the view is read.
    Live,
    /// From the records the store keeps, folded from each source's webhooks
    /// and reconciled with a listing.
    Kept,
}

/// A report: the view it shows, and when.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// The view.
    pub view: String,
    /// When each of its windows begins.
    pub schedule: ReportSchedule,
}

/// A rule: the view it watches, when it looks, how long a pull request
/// waits before each nudge (ADR-0056), and whether it plans re-runs of
/// flaky checks (ADR-0065).
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// The view it watches, and when its windows begin.
    pub watch: Report,
    /// How long a pull request waits before each nudge, where it nudges.
    pub stages: Option<Stages>,
    /// The most pull requests it nudges, and the most re-runs it plans, a
    /// window.
    pub most: usize,
    /// Whether it plans a re-run of each flaky check failing on its view's
    /// pull requests, announced to its private routes.
    pub rerun: bool,
    /// The most failed runs on one head a plan reads for one check, the
    /// newest first.
    pub runs: usize,
}

/// What a view does with a bot's pull requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[serde(rename = "Bots")]
pub(crate) enum BotsFile {
    /// Lists them with everyone's.
    Include,
    /// Leaves them out.
    Exclude,
    /// Lists only them.
    Only,
    /// Gathers them into batches of the same bump across repositories.
    Batch,
}

/// What a view groups its rows by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[serde(rename = "GroupBy")]
pub(crate) enum GroupByFile {
    /// Whose turn it is.
    Turn,
    /// The repository.
    Repository,
    /// The author.
    Author,
    /// Each area its changed paths fall in, a row in each of its areas'
    /// groups and one in none in a group of its own (ADR-0071).
    Area,
    /// Nothing: one group.
    Nothing,
}

/// What tells a view whose turn a pull request is, by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[serde(rename = "DecidingName")]
pub(crate) enum DecidingName {
    /// herald's own rules.
    Herald,
    /// The forge's own status, mapped as it is.
    Forge,
}

/// A label maintainers set, and the state it puts a pull request in.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "LabelledState")]
pub(crate) struct LabelledFile {
    /// The label, as the forge names it, ignoring case.
    pub(crate) label: String,
    /// The state.
    pub(crate) state: Turn,
}

/// The labels maintainers set, the first a pull request carries telling
/// its state.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "DecidingLabels")]
pub(crate) struct DecidingLabels {
    /// Each label and its state, in the order they are tried.
    #[schemars(length(min = 1))]
    pub(crate) labels: Vec<LabelledFile>,
}

/// What tells a view whose turn a pull request is: "herald", "forge", or
/// labels maintainers set.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(untagged)]
#[serde(rename = "Deciding")]
pub(crate) enum DecidingFile {
    /// herald's own rules, or the forge's own status.
    Named(DecidingName),
    /// The labels maintainers set.
    Labels(DecidingLabels),
}

/// herald's own rules, each on where absent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Rules")]
pub(crate) struct RulesFile {
    /// A draft waits on its author's readiness.
    pub(crate) draft: Option<bool>,
    /// One in the merge queue waits on the queue.
    pub(crate) queued: Option<bool>,
    /// One that conflicts waits on its author.
    pub(crate) conflict: Option<bool>,
    /// One a reviewer asked to change waits on its author.
    pub(crate) changes: Option<bool>,
    /// One whose check failed waits on its author.
    pub(crate) failed: Option<bool>,
    /// One whose checks are running waits on them.
    pub(crate) pending: Option<bool>,
    /// One waits on review until it is approved.
    pub(crate) approval: Option<bool>,
}

/// `[views.<name>]`: which open pull requests a report, `herald queue` and
/// every other surface shows, and how.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "View")]
pub(crate) struct ViewFile {
    /// The sources it reads, each naming one repository or an owner on a
    /// forge; every such source where absent.
    #[schemars(with = "Option<Vec<String>>")]
    pub(crate) sources: Option<Vec<Spanned<String>>>,
    /// The states it shows, every one where empty.
    #[serde(default)]
    pub(crate) states: Vec<Turn>,
    /// What it does with a bot's pull requests: "batch" where absent.
    pub(crate) bots: Option<BotsFile>,
    /// The repositories it shows, by full path, each a glob ignoring case;
    /// every one where empty.
    #[serde(default)]
    pub(crate) repositories: Vec<String>,
    /// The authors it shows, by login, each a glob ignoring case; every one
    /// where empty.
    #[serde(default)]
    pub(crate) authors: Vec<String>,
    /// The labels, any one of which a pull request it shows carries, each a
    /// glob ignoring case; every pull request where empty.
    #[serde(default)]
    pub(crate) labels: Vec<String>,
    /// The areas, any one of which a pull request it shows falls in, by
    /// name, each a glob ignoring case that matches an area one of its
    /// sources names; every pull request where empty (ADR-0071).
    #[serde(default)]
    pub(crate) areas: Vec<String>,
    /// How long one waits with nobody acting before it is stale: "14d"
    /// where absent.
    pub(crate) stale: Option<Duration>,
    /// Whether it shows only the stale.
    #[serde(default)]
    pub(crate) stale_only: bool,
    /// What it groups its rows by: "turn" where absent.
    pub(crate) group_by: Option<GroupByFile>,
    /// What tells whose turn each is: "herald" where absent, "forge", or
    /// `{ labels = [{ label = "...", state = "..." }] }`.
    pub(crate) deciding: Option<DecidingFile>,
    /// herald's own rules, each of which it may switch off.
    #[serde(default)]
    pub(crate) rules: RulesFile,
    /// Its own bump patterns, read before Dependabot's and Renovate's, such
    /// as "chore: bump {package} to {to}".
    #[serde(default)]
    pub(crate) bumps: Vec<String>,
    /// The most open pull requests it lists of each repository: 500 where
    /// absent.
    pub(crate) most_open: Option<NonZeroUsize>,
    /// The most it reads in full where each takes requests of its own, as
    /// on Forgejo and on GitHub without a token: 100 where absent; those
    /// past it read as unknown.
    pub(crate) most_pulls: Option<usize>,
    /// The task that decides each slot its rules leave untold, such as
    /// `{ turn = "whose", priority = "urgency" }`; nothing is asked of a
    /// model where absent.
    #[serde(default)]
    #[schemars(with = "BTreeMap<Slot, String>")]
    pub(crate) decide: BTreeMap<Slot, Spanned<String>>,
    /// Where it reads its pull requests: "auto" where absent, kept of each
    /// source that takes webhooks where the store keeps pull requests and
    /// live otherwise; "live" from the forge each time; or "kept" from the
    /// records the store keeps, folded from webhooks and reconciled with a
    /// listing.
    pub(crate) mode: Option<ViewMode>,
    /// How long kept mode waits at least before listing a source's open
    /// pull requests again, each page conditional on its last answer; every
    /// round where absent.
    pub(crate) reconcile: Option<Duration>,
    /// Whether a live read through a REST listing without a token, as of
    /// GitHub, GitHub Enterprise Server, GHE.com or Forgejo, takes who may
    /// see a repository from what the listing states of it, rather than
    /// taking each of its rows as private: false where absent. It never
    /// widens a kept record, a read with a token, a GitLab listing, or an
    /// owner's rows, which its membership record names.
    #[serde(default)]
    pub(crate) listed_visibility: bool,
    /// Whether it predicts conflicts: it reads the paths each pull
    /// request's head changes, at most 100 each, and each row names the
    /// open pull requests of its repository merging into its branch that
    /// change a path in common; false where absent. It costs a request a
    /// pull request read in full, but on GitHub with a token, which reads
    /// them with each pull request; a kept record keeps them until its head
    /// moves.
    #[serde(default)]
    pub(crate) conflicts: bool,
    /// How many open pull requests one check fails on before the base they
    /// merge into reads as broken: 3 where absent (ADR-0065).
    pub(crate) broken_at: Option<NonZeroUsize>,
    /// What makes a failing check a flaky candidate: failing then passing
    /// on one head at `least` times (1 where absent) within `window`
    /// ("7d" where absent), as kept records saw it.
    pub(crate) flaky: Option<FlakyFile>,
    /// The overviews it shows beside its rows, such as `["capacity"]`: none
    /// where absent (ADR-0066).
    #[serde(default)]
    pub(crate) overviews: Vec<OverviewName>,
    /// Which repositories its capacity overview reads: "shown", those it
    /// shows a pull request of, where absent; or "members", its owner's
    /// newest pushed, at most `capacity_repositories`, or where it filters
    /// its repositories by name the first by name of those it holds.
    pub(crate) capacity_from: Option<CapacityFrom>,
    /// The most members "members" reads, the newest pushed first, or the
    /// first by name where the view filters its repositories by name: 20
    /// where absent, from 1 to 100. Each costs two requests on GitHub and
    /// GitLab and one on Forgejo, and the push order one more.
    #[schemars(range(min = 1, max = herald_core::MOST_CAPACITY_REPOSITORIES))]
    pub(crate) capacity_repositories: Option<NonZeroUsize>,
    /// Which repositories its advisories overview reads: "owner", where
    /// absent, its organization's or group's own listings, and a user's
    /// members one by one, the newest pushed first; "shown", those it shows
    /// a pull request of; or "members", its owner's newest pushed. A view
    /// that filters its repositories by name reads those it holds one by
    /// one, the first by name. A one-repository source reads its
    /// repository.
    pub(crate) advisories_from: Option<AdvisoriesFromFile>,
    /// The most repositories its advisories overview reads one by one: 20
    /// where absent, from 1 to 100. Each costs three requests on GitHub
    /// and one on GitLab, and their order one more.
    #[schemars(range(min = 1, max = herald_core::MOST_ADVISORY_REPOSITORIES))]
    pub(crate) advisory_repositories: Option<NonZeroUsize>,
    /// The most open advisories its overview reads, a page of 100 a
    /// request: 1000 where absent, at most 10000. Past it the section says
    /// more were open.
    #[schemars(range(min = 1, max = herald_core::MOST_ADVISORIES_ALLOWED))]
    pub(crate) most_advisories: Option<NonZeroUsize>,
}

/// An overview a view shows beside its rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OverviewName {
    /// Its repositories' open security alerts, shown to private routes
    /// alone (ADR-0067).
    Advisories,
    /// Its repositories' CI runs queued and running, and those superseded.
    Capacity,
}

/// Which repositories an advisories overview reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AdvisoriesFromFile {
    /// Its organization's or group's own listings, and a user's members
    /// one by one, the newest pushed first.
    #[default]
    Owner,
    /// Those the view shows a pull request of.
    Shown,
    /// Its owner's members, the newest pushed first.
    Members,
}

/// Which repositories a capacity overview reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CapacityFrom {
    /// Those the view shows a pull request of.
    #[default]
    Shown,
    /// Its owner's members, the newest pushed first.
    Members,
}

/// What makes a failing check a flaky candidate.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Flaky")]
pub(crate) struct FlakyFile {
    /// How far back a check failing then passing on one head counts: "7d"
    /// where absent.
    pub(crate) window: Option<Duration>,
    /// How many times within the window make a candidate: 1 where absent.
    pub(crate) least: Option<NonZeroUsize>,
}

/// A day of the week, by its first three letters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[serde(rename = "Day")]
pub(crate) enum DayFile {
    /// Monday.
    Mon,
    /// Tuesday.
    Tue,
    /// Wednesday.
    Wed,
    /// Thursday.
    Thu,
    /// Friday.
    Fri,
    /// Saturday.
    Sat,
    /// Sunday.
    Sun,
}

impl DayFile {
    pub(crate) const fn weekday(self) -> Weekday {
        match self {
            Self::Mon => Weekday::Monday,
            Self::Tue => Weekday::Tuesday,
            Self::Wed => Weekday::Wednesday,
            Self::Thu => Weekday::Thursday,
            Self::Fri => Weekday::Friday,
            Self::Sat => Weekday::Saturday,
            Self::Sun => Weekday::Sunday,
        }
    }
}

/// `[reports.<name>]`: a view announced once a window, through the routes
/// naming the report among their sources.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "Report")]
pub(crate) struct ReportFile {
    /// The view it shows: "queue", every source's open pull requests, where
    /// absent.
    #[schemars(with = "Option<String>")]
    pub(crate) view: Option<Spanned<String>>,
    /// How long each window lasts within a day, from "1m" to "1d": "1d"
    /// where absent.
    #[schemars(with = "Option<Duration>")]
    pub(crate) every: Option<Spanned<Duration>>,
    /// The wall-clock time a day's first window begins at, as "HH:MM":
    /// "09:00" where absent.
    #[schemars(with = "Option<String>", example = ReportFile::AT)]
    pub(crate) at: Option<Spanned<String>>,
    /// The days windows begin on, every day where empty.
    #[serde(default)]
    pub(crate) days: Vec<DayFile>,
    /// The IANA time zone `at` and `days` are read in, such as
    /// "Europe/Amsterdam": "UTC" where absent.
    #[schemars(with = "Option<String>", example = ReportFile::ZONE)]
    pub(crate) zone: Option<Spanned<String>>,
}

impl ReportFile {
    /// The time the schema and the loader's errors show as an example.
    pub(crate) const AT: &str = "09:00";
    /// The zone the schema and the loader's errors show as an example.
    pub(crate) const ZONE: &str = "Europe/Amsterdam";
}

/// The keys of `[reports.<name>]`, which a board and a rule write too.
pub(crate) const REPORT_KEYS: [&str; 5] = ["view", "every", "at", "days", "zone"];

/// The keys of a route a board gives itself, and so names none of.
pub(crate) const BOARD_KEYS: [&str; 3] = ["id", "sources", "living"];

/// The keys of `[rules.<name>]` beside a report's.
pub(crate) const RULE_KEYS: [&str; 3] = ["stages", "most", "flaky"];

/// What `[rules.<name>]` writes beside the view it watches and when it
/// looks, which it writes as a report does.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "RuleStages")]
pub(crate) struct RuleFile {
    /// How long an open pull request waits, since anyone last acted on
    /// it, before each nudge, such as `["2d", "7d"]`: each fires once a
    /// spell of waiting.
    #[schemars(with = "Option<Vec<Duration>>", length(min = 1, max = herald_core::MOST_STAGES))]
    pub(crate) stages: Option<Spanned<Vec<Duration>>>,
    /// The most pull requests it nudges a window, the longest waiting
    /// first, and the most re-runs it plans: 20 where absent; the rest wait
    /// for the next window.
    pub(crate) most: Option<std::num::NonZeroUsize>,
    /// Whether it plans a re-run of each flaky check, as
    /// `{ rerun = true, mode = "plan" }`: each plan names its evidence and
    /// the exact call, announced once a run and head to the rule's private
    /// routes. A rule only plans; a re-run is made by an operator signed in
    /// through the forge, never by a rule on its own (ADR-0065).
    #[schemars(with = "Option<FlakyRuleFile>")]
    pub(crate) flaky: Option<Spanned<FlakyRuleFile>>,
}

/// What a rule does with a flaky check.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "FlakyRule")]
pub(crate) struct FlakyRuleFile {
    /// Whether it plans a re-run of each flaky check.
    pub(crate) rerun: bool,
    /// "plan", the only mode a rule takes: where absent too.
    pub(crate) mode: Option<RerunMode>,
    /// The most failed runs on one head a plan reads for one check, the
    /// newest first, from 1 to 100: 5 where absent. A plan takes one
    /// request for them and, on GitHub, one more for each.
    #[schemars(range(min = 1, max = herald_core::MOST_RUNS_ALLOWED))]
    pub(crate) runs: Option<NonZeroUsize>,
}

/// How a rule acts on a flaky check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RerunMode {
    /// It announces a plan.
    Plan,
    /// It would re-run, which only a signed-in operator does.
    Apply,
}

/// `[rules.<name>]`: a view watched once a window, and a nudge for each
/// pull request that waited past a stage, through the routes naming the
/// rule among their sources.
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
#[schemars(rename = "Rule")]
#[allow(dead_code, reason = "read only by the schema's derive")]
pub(crate) struct RuleSchema {
    #[serde(flatten)]
    watch: ReportFile,
    #[serde(flatten)]
    stages: RuleFile,
}
