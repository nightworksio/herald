//! `[views.*]`, `[reports.*]` (ADR-0055) and `[rules.*]` (ADR-0056): views
//! read before the reports and rules naming them, and those before the
//! routes naming them among their sources.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::ops::Range;

use herald_core::report::{DAY, SHORTEST};
use herald_core::{
    Bots, BumpPattern, Deciding, Duration, Glob, GroupBy, MOST_NUDGES, MOST_RUNS,
    MOST_RUNS_ALLOWED, MOST_STAGES, ReportSchedule, Rules, Slot, SourceName, Stages, Triage, View,
    zone,
};
use herald_kinds::{Most, Source};
use jiff::SignedDuration;
use jiff::civil::Time;
use toml::Spanned;

use super::boards::Boarded;
use super::{Loader, Value, list};
use crate::models::Task;
use crate::views::{
    BUILT_IN_VIEW, BotsFile, DecidingFile, DecidingName, GroupByFile, RULE_KEYS, Report,
    ReportFile, RerunMode, Rule, RuleFile, RulesFile, ViewConfig, ViewFile, ViewMode,
};

/// Why a rule may not re-run a check itself (ADR-0065).
const APPLIED: &str = "a rule only plans a re-run, so mode is \"plan\"; a re-run is made by an operator signed in through the forge, never by a rule on its own";

/// Why a rule that neither nudges nor plans is refused.
const RUNS: &str = "a plan reads from 1 to 100 failed runs on a head, such as runs = 5";
const IDLE: &str = "a rule nudges at its stages, plans re-runs of flaky checks with flaky = { rerun = true }, or both; this one does neither";

/// The names of `sources` that list their repositories' open pull requests.
fn queued(sources: &BTreeMap<SourceName, Source>) -> Vec<SourceName> {
    sources
        .iter()
        .filter(|(_, source)| source.queue_cost(Most::default()).is_some())
        .map(|(name, _)| name.clone())
        .collect()
}

impl BotsFile {
    const fn bots(self) -> Bots {
        match self {
            Self::Include => Bots::Include,
            Self::Exclude => Bots::Exclude,
            Self::Only => Bots::Only,
            Self::Batch => Bots::Batch,
        }
    }
}

impl GroupByFile {
    const fn group_by(self) -> GroupBy {
        match self {
            Self::Turn => GroupBy::Turn,
            Self::Repository => GroupBy::Repository,
            Self::Author => GroupBy::Author,
            Self::Nothing => GroupBy::Nothing,
        }
    }
}

impl DecidingFile {
    fn deciding(self) -> Deciding {
        match self {
            Self::Named(DecidingName::Herald) => Deciding::Herald,
            Self::Named(DecidingName::Forge) => Deciding::Forge,
            Self::Labels(labels) => Deciding::Labels(
                labels
                    .labels
                    .into_iter()
                    .map(|labelled| (labelled.label, labelled.state))
                    .collect(),
            ),
        }
    }
}

impl RulesFile {
    fn rules(self) -> Rules {
        let shipped = Rules::default();
        Rules {
            draft: self.draft.unwrap_or(shipped.draft),
            queued: self.queued.unwrap_or(shipped.queued),
            conflict: self.conflict.unwrap_or(shipped.conflict),
            changes: self.changes.unwrap_or(shipped.changes),
            failed: self.failed.unwrap_or(shipped.failed),
            pending: self.pending.unwrap_or(shipped.pending),
            approval: self.approval.unwrap_or(shipped.approval),
        }
    }
}

impl Loader<'_> {
    /// Each of `texts`, the globs `path` lists, where every one is one.
    fn globs(&mut self, texts: &[String], (path, span): (&str, &Range<usize>)) -> Vec<Glob> {
        let mut globs = Vec::new();
        for (index, text) in texts.iter().enumerate() {
            match Glob::new(text) {
                Ok(glob) => globs.push(glob),
                Err(error) => self.error(span, &format!("{path}[{index}]"), error.to_string()),
            }
        }
        globs
    }

    /// The sources a view at `path` reads: those it names, each one of
    /// `sources` that lists its repository's open pull requests, or every
    /// such source where it names none.
    fn view_sources(
        &mut self,
        given: Option<Vec<Spanned<String>>>,
        path: &str,
        sources: &BTreeMap<SourceName, Source>,
    ) -> Vec<SourceName> {
        let queued = queued(sources);
        let Some(given) = given else {
            return queued;
        };
        let mut read = Vec::new();
        for source in given {
            let name = SourceName::new(source.get_ref());
            if queued.contains(&name) {
                read.push(name);
                continue;
            }
            let names: Vec<&str> = queued.iter().map(SourceName::as_str).collect();
            let why = if sources.contains_key(&name) {
                "names no repository or owner on a forge, so it lists no pull requests a view reads"
            } else {
                "is no source"
            };
            self.error(
                &source.span(),
                &format!("{path}.sources"),
                format!(
                    "\"{}\" {why}; the sources a view reads are {}",
                    name.as_str(),
                    list(&names)
                ),
            );
        }
        read
    }

    /// The task of `tasks` deciding each slot `decide` names, at `path`,
    /// where it offers options the slot may settle on.
    fn deciding(
        &mut self,
        decide: BTreeMap<Slot, Spanned<String>>,
        path: &str,
        tasks: &BTreeMap<String, Task>,
    ) -> BTreeMap<Slot, String> {
        let mut deciding = BTreeMap::new();
        for (slot, task) in decide {
            let key = format!("{path}.decide.{}", slot.name());
            match tasks.get(task.get_ref()) {
                None => {
                    let names: Vec<&str> = tasks.keys().map(String::as_str).collect();
                    self.error(
                        &task.span(),
                        &key,
                        format!(
                            "no task is named \"{}\"; the tasks are {}",
                            task.get_ref(),
                            list(&names)
                        ),
                    );
                }
                Some(asked) if !slot.takes(&asked.options) => self.error(
                    &task.span(),
                    &key,
                    format!(
                        "the task \"{}\" decides whose turn it is, so each of its options is a state but unknown, such as waiting-on-review",
                        task.get_ref()
                    ),
                ),
                Some(_) => {
                    deciding.insert(slot, task.into_inner());
                }
            }
        }
        deciding
    }

    /// The view `file` writes at `path`, its table at `span`.
    fn view(
        &mut self,
        file: ViewFile,
        (path, span): (&str, &Range<usize>),
        (sources, tasks): (&BTreeMap<SourceName, Source>, &BTreeMap<String, Task>),
    ) -> ViewConfig {
        let shipped = View::default();
        let mut bumps = Vec::new();
        for (index, text) in file.bumps.iter().enumerate() {
            match BumpPattern::new(text) {
                Ok(pattern) => bumps.push(pattern),
                Err(error) => {
                    self.error(span, &format!("{path}.bumps[{index}]"), error.to_string());
                }
            }
        }
        let view = View {
            states: file.states,
            bots: file.bots.map_or(shipped.bots, BotsFile::bots),
            repositories: self.globs(&file.repositories, (&format!("{path}.repositories"), span)),
            authors: self.globs(&file.authors, (&format!("{path}.authors"), span)),
            labels: self.globs(&file.labels, (&format!("{path}.labels"), span)),
            stale: file.stale.map_or(shipped.stale, |stale| stale.0),
            stale_only: file.stale_only,
            group_by: file
                .group_by
                .map_or(shipped.group_by, GroupByFile::group_by),
            deciding: file
                .deciding
                .map_or(shipped.deciding, DecidingFile::deciding),
            rules: file.rules.rules(),
            bumps,
            triage: Triage {
                broken_at: file
                    .broken_at
                    .map_or(shipped.triage.broken_at, NonZeroUsize::get),
                flaky_window: file
                    .flaky
                    .as_ref()
                    .and_then(|flaky| flaky.window)
                    .map_or(shipped.triage.flaky_window, |window| window.0),
                flaky_least: file
                    .flaky
                    .as_ref()
                    .and_then(|flaky| flaky.least)
                    .map_or(shipped.triage.flaky_least, NonZeroUsize::get),
            },
        };
        let shipped = Most::default();
        ViewConfig {
            view,
            decide: self.deciding(file.decide, path, tasks),
            sources: self.view_sources(file.sources, path, sources),
            most: Most {
                open: file
                    .most_open
                    .map_or(shipped.open, std::num::NonZeroUsize::get),
                pulls: file.most_pulls.unwrap_or(shipped.pulls),
            },
            mode: file.mode.unwrap_or_default(),
            reconcile: file.reconcile.map(|every| every.0),
            stated: file.listed_visibility,
            conflicts: file.conflicts,
        }
    }

    /// Each view of `[views]`, and the built-in `queue` over every source
    /// that lists its repository's open pull requests where none replaces
    /// it.
    pub(super) fn views(
        &mut self,
        value: Option<Value<'_>>,
        sources: &BTreeMap<SourceName, Source>,
        tasks: &BTreeMap<String, Task>,
    ) -> BTreeMap<String, ViewConfig> {
        let mut views = BTreeMap::from([(
            BUILT_IN_VIEW.to_owned(),
            ViewConfig {
                view: View::default(),
                sources: queued(sources),
                most: Most::default(),
                decide: BTreeMap::new(),
                mode: ViewMode::default(),
                reconcile: None,
                stated: false,
                conflicts: false,
            },
        )]);
        for (name, value) in self.named(value, "views") {
            let path = format!("views.{name}");
            let span = value.span();
            if let Some(file) = self.typed::<ViewFile>(value, &path) {
                let view = self.view(file, (&path, &span), (sources, tasks));
                views.insert(name, view);
            }
        }
        views
    }

    /// The time `at`, at `path`, writes as "HH:MM".
    fn report_at(&mut self, at: &Spanned<String>, path: &str) -> Option<Time> {
        let read = at
            .get_ref()
            .split_once(':')
            .filter(|(hours, minutes)| hours.len() == 2 && minutes.len() == 2)
            .and_then(|(hours, minutes)| Some((hours.parse().ok()?, minutes.parse().ok()?)))
            .and_then(|(hours, minutes)| Time::new(hours, minutes, 0, 0).ok());
        if read.is_none() {
            self.error(
                &at.span(),
                &format!("{path}.at"),
                format!(
                    "a report begins at a wall-clock time written \"HH:MM\", such as at = \"{}\"",
                    ReportFile::AT
                ),
            );
        }
        read
    }

    /// The schedule `file` writes at `path`, where it is sound.
    fn report_schedule(&mut self, file: &ReportFile, path: &str) -> Option<ReportSchedule> {
        let shipped = ReportSchedule::default();
        let mut sound = true;
        let every = file.every.as_ref().map_or(shipped.every, |every| {
            let length = every.get_ref().0;
            if !(SHORTEST..=DAY).contains(&length) {
                self.error(
                    &every.span(),
                    &format!("{path}.every"),
                    "a report's window lasts from \"1m\" to \"1d\"",
                );
                sound = false;
            }
            length
        });
        let at = match &file.at {
            Some(at) => self.report_at(at, path),
            None => Some(shipped.at),
        };
        let zone = file.zone.as_ref().map_or(Some(shipped.zone), |named| {
            zone(named.get_ref())
                .inspect_err(|_| {
                    self.error(
                        &named.span(),
                        &format!("{path}.zone"),
                        format!(
                            "there is no time zone \"{}\"; name one as the IANA database does, such as zone = \"{}\", or \"UTC\"",
                            named.get_ref(),
                            ReportFile::ZONE
                        ),
                    );
                })
                .ok()
        });
        let (true, Some(at), Some(zone)) = (sound, at, zone) else {
            return None;
        };
        Some(ReportSchedule {
            every,
            at,
            days: file.days.iter().map(|day| day.weekday()).collect(),
            zone,
        })
    }

    /// Whether `name`, written at `span` of `path`, is no source's of
    /// `sources`, no app's of `apps` and none of `taken`'s, as a route
    /// names sources, apps, reports and rules alike.
    fn unshared(
        &mut self,
        (name, path, span): (&str, &str, &Range<usize>),
        sources: &BTreeMap<SourceName, Source>,
        apps: &BTreeSet<&str>,
        taken: &[&str],
    ) -> bool {
        let shared = sources.contains_key(&SourceName::new(name))
            || apps.contains(name)
            || taken.contains(&name);
        if shared {
            self.error(
                span,
                path,
                format!(
                    "a source, app, report or rule is named \"{name}\" too; a route names each alike, so give each its own name"
                ),
            );
        }
        !shared
    }

    /// The report `file` writes at `path`, showing one of `views`, where
    /// it is sound.
    fn report(
        &mut self,
        file: &ReportFile,
        path: &str,
        views: &BTreeMap<String, ViewConfig>,
    ) -> Option<Report> {
        let view = file
            .view
            .as_ref()
            .map_or_else(|| BUILT_IN_VIEW.to_owned(), |view| view.get_ref().clone());
        let mut sound = true;
        if let (Some(named), false) = (&file.view, views.contains_key(&view)) {
            let names: Vec<&str> = views.keys().map(String::as_str).collect();
            self.error(
                &named.span(),
                &format!("{path}.view"),
                format!(
                    "no view is named \"{view}\"; the views are {}",
                    list(&names)
                ),
            );
            sound = false;
        }
        let schedule = self.report_schedule(file, path);
        let (true, Some(schedule)) = (sound, schedule) else {
            return None;
        };
        Some(Report { view, schedule })
    }

    /// Each report of `[reports]`, and of each board of `boarded`, showing
    /// one of `views`, its name one no source of `sources`, no app of
    /// `apps` and no other report has.
    pub(super) fn reports(
        &mut self,
        value: Option<Value<'_>>,
        boarded: Vec<Boarded<'_>>,
        views: &BTreeMap<String, ViewConfig>,
        (sources, apps): (&BTreeMap<SourceName, Source>, &BTreeSet<&str>),
    ) -> BTreeMap<String, Report> {
        let written: Vec<Boarded<'_>> = self
            .named(value, "reports")
            .into_iter()
            .map(|(name, value)| (format!("reports.{name}"), name, value))
            .chain(boarded)
            .collect();
        let mut reports = BTreeMap::new();
        let mut named: Vec<String> = Vec::new();
        for (path, name, value) in written {
            let span = value.span();
            let Some(file) = self.typed::<ReportFile>(value, &path) else {
                continue;
            };
            let taken: Vec<&str> = named.iter().map(String::as_str).collect();
            let unshared = self.unshared((&name, &path, &span), sources, apps, &taken);
            named.push(name.clone());
            if let (true, Some(report)) = (unshared, self.report(&file, &path, views)) {
                reports.insert(name, report);
            }
        }
        reports
    }

    /// The stages `stages`, written at `path`, where they are sound.
    fn stages(&mut self, stages: &Spanned<Vec<Duration>>, path: &str) -> Option<Stages> {
        let written: Vec<SignedDuration> = stages.get_ref().iter().map(|stage| stage.0).collect();
        let sound = Stages::new(written);
        if sound.is_none() {
            self.error(
                &stages.span(),
                &format!("{path}.stages"),
                format!(
                    "a rule names from 1 to {MOST_STAGES} stages, each a whole second or longer and none twice, such as stages = [\"2d\", \"7d\"]"
                ),
            );
        }
        sound
    }

    /// Whether the rule `file` writes at `path`, spanning `span`, plans
    /// re-runs, where it does something at all: it nudges, plans, or both,
    /// and plans only, a re-run being an operator's (ADR-0065).
    fn reruns(
        &mut self,
        file: &RuleFile,
        (path, span): (&str, &Range<usize>),
    ) -> Option<(bool, usize)> {
        let (rerun, runs) = match &file.flaky {
            Some(flaky) if flaky.get_ref().mode == Some(RerunMode::Apply) => {
                self.error(
                    &flaky.span(),
                    &format!("{path}.flaky.mode"),
                    APPLIED.to_owned(),
                );
                return None;
            }
            Some(flaky) => {
                let runs = flaky.get_ref().runs.map_or(MOST_RUNS, NonZeroUsize::get);
                if runs > MOST_RUNS_ALLOWED {
                    self.error(
                        &flaky.span(),
                        &format!("{path}.flaky.runs"),
                        RUNS.to_owned(),
                    );
                    return None;
                }
                (flaky.get_ref().rerun, runs)
            }
            None => (false, MOST_RUNS),
        };
        if file.stages.is_none() && !rerun {
            self.error(span, path, IDLE.to_owned());
            return None;
        }
        Some((rerun, runs))
    }

    /// Each rule of `[rules]`, watching one of `views`, its name one no
    /// source of `sources`, no app of `apps`, no report of `reports` and no
    /// other rule has.
    pub(super) fn rules(
        &mut self,
        value: Option<Value<'_>>,
        views: &BTreeMap<String, ViewConfig>,
        (sources, apps): (&BTreeMap<SourceName, Source>, &BTreeSet<&str>),
        reports: &BTreeMap<String, Report>,
    ) -> BTreeMap<String, Rule> {
        let mut rules = BTreeMap::new();
        let mut named: Vec<String> = reports.keys().cloned().collect();
        for (name, value) in self.named(value, "rules") {
            let path = format!("rules.{name}");
            let span = value.span();
            let Some((own, at, watch)) = self.parted(value, &path, &RULE_KEYS) else {
                continue;
            };
            let watch = Spanned::new(at, toml::de::DeValue::Table(watch));
            let file = self.typed::<RuleFile>(own, &path);
            let watch = self.typed::<ReportFile>(watch, &path);
            let taken: Vec<&str> = named.iter().map(String::as_str).collect();
            let unshared = self.unshared((&name, &path, &span), sources, apps, &taken);
            named.push(name.clone());
            let (Some(file), Some(watch)) = (file, watch) else {
                continue;
            };
            let stages = file
                .stages
                .as_ref()
                .map(|stages| self.stages(stages, &path));
            let rerun = self.reruns(&file, (&path, &span));
            let watch = self.report(&watch, &path, views);
            let sound = stages.as_ref().is_none_or(Option::is_some);
            if let (true, true, Some((rerun, runs)), Some(watch)) = (unshared, sound, rerun, watch)
            {
                let most = file.most.map_or(MOST_NUDGES, NonZeroUsize::get);
                rules.insert(
                    name,
                    Rule {
                        watch,
                        stages: stages.flatten(),
                        most,
                        rerun,
                        runs,
                    },
                );
            }
        }
        rules
    }
}
