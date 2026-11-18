//! `[views.*]` and `[reports.*]` (ADR-0055): views read before the reports
//! naming them, reports before the routes naming them among their sources.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use herald_core::report::{DAY, SHORTEST};
use herald_core::{
    Bots, BumpPattern, Deciding, Glob, GroupBy, ReportSchedule, Rules, SourceName, View, zone,
};
use herald_kinds::{Most, Source};
use jiff::civil::Time;
use toml::Spanned;

use super::{Loader, Value, list};
use crate::views::{
    BUILT_IN_VIEW, BotsFile, DecidingFile, DecidingName, GroupByFile, Report, ReportFile,
    RulesFile, ViewConfig, ViewFile,
};

/// The names of `sources` that list their repository's open pull requests.
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
                "names no one repository on a forge, so it lists no pull requests a view reads"
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

    /// The view `file` writes at `path`, its table at `span`.
    fn view(
        &mut self,
        file: ViewFile,
        (path, span): (&str, &Range<usize>),
        sources: &BTreeMap<SourceName, Source>,
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
        };
        let shipped = Most::default();
        ViewConfig {
            view,
            sources: self.view_sources(file.sources, path, sources),
            most: Most {
                open: file
                    .most_open
                    .map_or(shipped.open, std::num::NonZeroUsize::get),
                pulls: file.most_pulls.unwrap_or(shipped.pulls),
            },
        }
    }

    /// Each view of `[views]`, and the built-in `queue` over every source
    /// that lists its repository's open pull requests where none replaces
    /// it.
    pub(super) fn views(
        &mut self,
        value: Option<Value<'_>>,
        sources: &BTreeMap<SourceName, Source>,
    ) -> BTreeMap<String, ViewConfig> {
        let mut views = BTreeMap::from([(
            BUILT_IN_VIEW.to_owned(),
            ViewConfig {
                view: View::default(),
                sources: queued(sources),
                most: Most::default(),
            },
        )]);
        for (name, value) in self.named(value, "views") {
            let path = format!("views.{name}");
            let span = value.span();
            if let Some(file) = self.typed::<ViewFile>(value, &path) {
                let view = self.view(file, (&path, &span), sources);
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

    /// Each report of `[reports]`, showing one of `views`, its name one no
    /// source of `sources` and no app of `apps` has.
    pub(super) fn reports(
        &mut self,
        value: Option<Value<'_>>,
        views: &BTreeMap<String, ViewConfig>,
        sources: &BTreeMap<SourceName, Source>,
        apps: &BTreeSet<&str>,
    ) -> BTreeMap<String, Report> {
        let mut reports = BTreeMap::new();
        for (name, value) in self.named(value, "reports") {
            let path = format!("reports.{name}");
            let span = value.span();
            let Some(file) = self.typed::<ReportFile>(value, &path) else {
                continue;
            };
            let mut sound = true;
            if sources.contains_key(&SourceName::new(&name)) || apps.contains(name.as_str()) {
                self.error(
                    &span,
                    &path,
                    format!(
                        "a source or app is named \"{name}\" too; a route names sources, apps and reports alike, so give each its own name"
                    ),
                );
                sound = false;
            }
            let view = file
                .view
                .as_ref()
                .map_or_else(|| BUILT_IN_VIEW.to_owned(), |view| view.get_ref().clone());
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
            let schedule = self.report_schedule(&file, &path);
            if let (true, Some(schedule)) = (sound, schedule) {
                reports.insert(name, Report { view, schedule });
            }
        }
        reports
    }
}
