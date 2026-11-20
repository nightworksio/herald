//! `[views.*]` and `[reports.*]` as the file writes them, and as a config
//! holds them (ADR-0055).

use std::num::NonZeroUsize;

use std::collections::BTreeMap;

use herald_core::{Duration, ReportSchedule, Slot, SourceName, Stages, Turn, View};
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
pub(crate) const QUEUE_BODY: &str = "{% for group in view.groups %}**{{ group.name or \"everyone\" }}**\n{% for row in group.rows %}- [{{ row.repository }}#{{ row.number }}]({{ row.url }}) {{ row.title }}: {{ row.reason }}, {{ row.waited }}{% if row.stale %}, stale{% endif %}\n{% endfor %}\n{% endfor %}{% if view.batches %}**bumps**\n{% for batch in view.batches %}- {{ batch.package or \"unread\" }}{% if batch.to %} to {{ batch.to }}{% endif %}: {{ batch.rows | length }} open, {{ batch.green }} green, {{ batch.failing }} failing\n{% endfor %}{% endif %}{% if not view.total %}Nothing is open.{% endif %}";

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

/// A rule: the view it watches, when it looks, and how long a pull
/// request waits before each nudge (ADR-0056).
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// The view it watches, and when its windows begin.
    pub watch: Report,
    /// How long a pull request waits before each nudge.
    pub stages: Stages,
    /// The most pull requests it nudges a window.
    pub most: usize,
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
pub(crate) const RULE_KEYS: [&str; 2] = ["stages", "most"];

/// What `[rules.<name>]` writes beside the view it watches and when it
/// looks, which it writes as a report does.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(rename = "RuleStages")]
pub(crate) struct RuleFile {
    /// How long an open pull request waits, since anyone last acted on
    /// it, before each nudge, such as `["2d", "7d"]`: each fires once a
    /// spell of waiting.
    #[schemars(with = "Vec<Duration>", length(min = 1, max = herald_core::MOST_STAGES))]
    pub(crate) stages: Spanned<Vec<Duration>>,
    /// The most pull requests it nudges a window, the longest waiting
    /// first: 20 where absent; the rest wait for the next window.
    pub(crate) most: Option<std::num::NonZeroUsize>,
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
