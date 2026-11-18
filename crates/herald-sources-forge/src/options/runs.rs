//! The keys that choose a forge source's runs.

use herald_core::{Glob, Signal};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{SourceOptions, globs};
use crate::Forge;

/// What a run is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RunFrom {
    /// The forge's own CI: GitHub Actions' workflow runs, GitLab's
    /// pipelines and Forgejo Actions' runs.
    Actions,
    /// GitHub's check suites and commit statuses, which other CI services
    /// report through; GitHub Actions' own check suites are read as
    /// workflow runs.
    Checks,
}

impl RunFrom {
    /// Its name, as `runs.from` writes it and a run's `from` attribute
    /// holds it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Actions => "actions",
            Self::Checks => "checks",
        }
    }
}

/// What a run did that is announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunAction {
    /// It was asked for, and waits to start.
    Requested,
    /// It started.
    InProgress,
    /// It finished.
    Completed,
}

impl RunAction {
    /// Its name, as `runs.actions` writes it and a run's `action`
    /// attribute holds it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }
}

/// How a finished run ended, in GitHub's words, which every forge's run is
/// read as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Conclusion {
    /// It passed.
    Success,
    /// It failed: GitLab's `failed`, a commit status's `failure` or
    /// `error`.
    Failure,
    /// It was cancelled: GitLab's `canceled`.
    Cancelled,
    /// It ran out of time.
    TimedOut,
    /// It was skipped.
    Skipped,
    /// It waits for someone: GitLab's `manual`.
    ActionRequired,
    /// It finished neither passing nor failing.
    Neutral,
    /// It failed before it started.
    StartupFailure,
    /// GitHub gave up on it.
    Stale,
}

impl Conclusion {
    /// Every conclusion.
    pub const ALL: [Self; 9] = [
        Self::Success,
        Self::Failure,
        Self::Cancelled,
        Self::TimedOut,
        Self::Skipped,
        Self::ActionRequired,
        Self::Neutral,
        Self::StartupFailure,
        Self::Stale,
    ];

    /// Its name, as `runs.conclusions` writes it and a run's `conclusion`
    /// attribute holds it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Skipped => "skipped",
            Self::ActionRequired => "action_required",
            Self::Neutral => "neutral",
            Self::StartupFailure => "startup_failure",
            Self::Stale => "stale",
        }
    }

    /// The conclusion GitHub names `name`, where it names one.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|conclusion| conclusion.name() == name)
    }
}

/// Which runs a source announces, where its signals take "run".
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Runs {
    /// What runs are read from: "actions", the forge's own CI, and on
    /// GitHub "checks", the check suites and commit statuses other CI
    /// services report; `["actions"]` where absent.
    pub from: Option<Vec<RunFrom>>,
    /// What a run did that is announced: `requested`, `in_progress`,
    /// `completed`; `["completed"]` where absent.
    pub actions: Option<Vec<RunAction>>,
    /// How a finished run ended for it to be announced; every conclusion
    /// where empty.
    #[serde(default)]
    pub conclusions: Vec<Conclusion>,
    /// The branches or tags runs are announced of, as globs as `include`
    /// writes them; every one where empty. One name with no glob in it is
    /// asked of the forge, so a poll reads only its runs.
    #[serde(default)]
    pub branches: Vec<String>,
    /// What started the runs announced, as the forge names it, such as
    /// `push` or `schedule`; GitLab's `merge_request_event` is written
    /// `pull_request`. Every trigger where empty.
    #[serde(default)]
    pub triggers: Vec<String>,
    /// The workflows, pipelines, check suites' apps or status contexts
    /// whose runs are announced, as globs as `include` writes them; every
    /// one where empty.
    #[serde(default)]
    pub workflows: Vec<String>,
}

/// The actions announced where `runs.actions` is absent.
pub const RUN_ACTIONS: [RunAction; 1] = [RunAction::Completed];
/// What runs are read from where `runs.from` is absent.
pub const RUN_FROM: [RunFrom; 1] = [RunFrom::Actions];

/// Which runs a source announces, read from its `runs` and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunChoice {
    pub(crate) from: Vec<RunFrom>,
    pub(crate) actions: Vec<RunAction>,
    pub(crate) conclusions: Vec<Conclusion>,
    pub(crate) branches: Vec<Glob>,
    pub(crate) triggers: Vec<String>,
    pub(crate) workflows: Vec<Glob>,
}

impl Default for RunChoice {
    fn default() -> Self {
        Self {
            from: RUN_FROM.to_vec(),
            actions: RUN_ACTIONS.to_vec(),
            conclusions: Vec::new(),
            branches: Vec::new(),
            triggers: Vec::new(),
            workflows: Vec::new(),
        }
    }
}

impl RunChoice {
    /// Whether it reads runs from `from`.
    pub(crate) fn reads(&self, from: RunFrom) -> bool {
        self.from.contains(&from)
    }

    /// Whether it announces a run of `branch` and `workflow`, started by
    /// `trigger`, that did `action` and ended with `conclusion` where it
    /// finished.
    pub(crate) fn takes(
        &self,
        (action, conclusion): (RunAction, Option<Conclusion>),
        (branch, trigger, workflow): (Option<&str>, Option<&str>, &str),
    ) -> bool {
        let globbed = |globs: &[Glob], name: Option<&str>| {
            globs.is_empty() || name.is_some_and(|name| globs.iter().any(|glob| glob.matches(name)))
        };
        self.actions.contains(&action)
            && (self.conclusions.is_empty()
                || conclusion.is_some_and(|conclusion| self.conclusions.contains(&conclusion)))
            && globbed(&self.branches, branch)
            && globbed(&self.workflows, Some(workflow))
            && (self.triggers.is_empty()
                || trigger.is_some_and(|trigger| {
                    self.triggers
                        .iter()
                        .any(|taken| taken.eq_ignore_ascii_case(trigger))
                }))
    }

    /// The one action it announces, where it announces one.
    pub(crate) fn only_action(&self) -> Option<RunAction> {
        only(&self.actions).copied()
    }

    /// The one conclusion it announces, where it announces one.
    pub(crate) fn only_conclusion(&self) -> Option<Conclusion> {
        only(&self.conclusions).copied()
    }

    /// The one trigger it announces, where it announces one.
    pub(crate) fn only_trigger(&self) -> Option<&str> {
        only(&self.triggers).map(String::as_str)
    }

    /// The one branch it announces, where it names one with no glob in it.
    pub(crate) fn only_branch(&self) -> Option<&str> {
        only(&self.branches)
            .filter(|branch| branch.specificity().0)
            .map(Glob::as_str)
    }
}

/// The one entry of `list`, where it holds one.
fn only<T>(list: &[T]) -> Option<&T> {
    match list {
        [one] => Some(one),
        _ => None,
    }
}

impl SourceOptions {
    /// Which runs the source announces.
    pub(crate) fn run_choice(&self) -> RunChoice {
        let Some(runs) = &self.runs else {
            return RunChoice::default();
        };
        RunChoice {
            from: runs.from.clone().unwrap_or_else(|| RUN_FROM.to_vec()),
            actions: runs.actions.clone().unwrap_or_else(|| RUN_ACTIONS.to_vec()),
            conclusions: runs.conclusions.clone(),
            branches: globs(&runs.branches).unwrap_or_default(),
            triggers: runs.triggers.clone(),
            workflows: globs(&runs.workflows).unwrap_or_default(),
        }
    }

    /// Whether `runs` is sound on `forge`, and given only where some
    /// repository the source takes announces runs.
    pub(super) fn runs_check(&self, forge: Forge) -> Result<(), String> {
        let Some(runs) = &self.runs else {
            return Ok(());
        };
        if !self.may_take(Signal::Run) {
            return Err(
                "runs chooses runs, which no repository the source takes announces; add \"run\" to signals, or remove runs".to_owned(),
            );
        }
        let lists = [
            ("from", runs.from.as_ref().is_some_and(Vec::is_empty)),
            ("actions", runs.actions.as_ref().is_some_and(Vec::is_empty)),
        ];
        if let Some((key, _)) = lists.into_iter().find(|(_, empty)| *empty) {
            return Err(format!(
                "runs.{key} names nothing, so no run would be announced; name one, or remove runs.{key}"
            ));
        }
        if forge != Forge::GitHub
            && runs
                .from
                .as_ref()
                .is_some_and(|from| from.contains(&RunFrom::Checks))
        {
            return Err(format!(
                "runs.from \"checks\" reads GitHub's check suites and commit statuses, which {} has none of; read its runs from \"actions\"",
                forge.kind()
            ));
        }
        let finished = runs
            .actions
            .as_ref()
            .is_none_or(|actions| actions.contains(&RunAction::Completed));
        if !runs.conclusions.is_empty() && !finished {
            return Err(
                "runs.conclusions chooses how finished runs ended, and runs.actions announces none finishing; add \"completed\" to runs.actions, or remove runs.conclusions".to_owned(),
            );
        }
        for (key, written) in [("branches", &runs.branches), ("workflows", &runs.workflows)] {
            globs(written).map_err(|error| format!("runs.{key}: {error}"))?;
        }
        Ok(())
    }
}
