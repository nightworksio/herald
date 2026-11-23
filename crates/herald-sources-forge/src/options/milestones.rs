//! The keys that choose a forge source's milestones (ADR-0079).

use std::num::NonZeroUsize;

use herald_core::{Glob, Signal};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{SourceOptions, globs};
use crate::Forge;

/// What a milestone did that is announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MilestoneAction {
    /// It was created.
    Opened,
    /// It was closed.
    Closed,
    /// It was opened again after it closed.
    Reopened,
    /// It was deleted, which withdraws its announcement from each route
    /// that withdraws (ADR-0048).
    Deleted,
    /// Its due day ended while it was open, as a poll finds by its own
    /// clock in UTC; no forge sends it (ADR-0080).
    Overdue,
}

impl MilestoneAction {
    /// Its name, as `milestones.actions` writes it and a milestone's
    /// `action` attribute holds it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Opened => "opened",
            Self::Closed => "closed",
            Self::Reopened => "reopened",
            Self::Deleted => "deleted",
            Self::Overdue => "overdue",
        }
    }
}

/// Which milestones a source announces, where its signals take
/// "milestone".
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Milestones {
    /// What a milestone did that is announced: `opened`, `closed`,
    /// `reopened`, `deleted`, `overdue` (found only by a poll); `closed`
    /// where absent.
    pub actions: Option<Vec<MilestoneAction>>,
    /// The titles, as globs as `include` writes them, one of which a
    /// milestone announced matches; every milestone where empty.
    #[serde(default)]
    pub titles: Vec<String>,
    /// Whether the milestones of the GitLab group holding the project are
    /// read beside the project's own; false where absent.
    pub group: Option<bool>,
    /// Whether a milestone names how many of its issues are open and
    /// closed, as its forge counts them; true where absent. On GitLab each
    /// announced milestone costs one request more.
    pub counts: Option<bool>,
    /// The most open milestones of a repository a poll follows, in the
    /// order its forge lists them; 100 where absent, at most 1000.
    pub most: Option<NonZeroUsize>,
}

/// The actions announced where `milestones.actions` is absent.
pub const MILESTONE_ACTIONS: [MilestoneAction; 1] = [MilestoneAction::Closed];
/// The most open milestones a poll follows where `milestones.most` is
/// absent.
pub const MOST_MILESTONES: NonZeroUsize = NonZeroUsize::new(100).unwrap();
/// The most `milestones.most` may be.
pub const MOST_MILESTONES_ALLOWED: usize = 1_000;

/// Which milestones a source announces, read from its `milestones`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MilestoneChoice {
    pub(crate) actions: Vec<MilestoneAction>,
    pub(crate) titles: Vec<Glob>,
    pub(crate) group: bool,
    pub(crate) counts: bool,
    pub(crate) most: usize,
}

impl MilestoneChoice {
    /// Whether it announces a milestone titled `title` that did `action`.
    pub(crate) fn takes(&self, action: MilestoneAction, title: &str) -> bool {
        self.actions.contains(&action)
            && (self.titles.is_empty() || self.titles.iter().any(|glob| glob.matches(title)))
    }
}

impl SourceOptions {
    /// Which milestones the source announces.
    pub(crate) fn milestone_choice(&self) -> MilestoneChoice {
        let milestones = self.milestones.clone().unwrap_or_default();
        MilestoneChoice {
            actions: milestones
                .actions
                .unwrap_or_else(|| MILESTONE_ACTIONS.to_vec()),
            titles: globs(&milestones.titles).unwrap_or_default(),
            group: milestones.group.unwrap_or_default(),
            counts: milestones.counts.unwrap_or(true),
            most: milestones.most.unwrap_or(MOST_MILESTONES).get(),
        }
    }

    /// Whether `milestones` is sound on `forge`, and given only where some
    /// repository the source takes announces milestones.
    pub(super) fn milestones_check(&self, forge: Forge) -> Result<(), String> {
        let Some(milestones) = &self.milestones else {
            return Ok(());
        };
        if milestones.group.is_some() && forge != Forge::GitLab {
            return Err(format!(
                "milestones.group reads a GitLab group's milestones, which {} has none of; remove it",
                forge.kind()
            ));
        }
        if !self.may_take(Signal::Milestone) {
            return Err(
                "milestones chooses milestones, which no repository the source takes announces; add \"milestone\" to signals, or remove milestones".to_owned(),
            );
        }
        if milestones.actions.as_ref().is_some_and(Vec::is_empty) {
            return Err(
                "milestones.actions names nothing, so no milestone would be announced; name one, or remove milestones.actions".to_owned(),
            );
        }
        if milestones
            .most
            .is_some_and(|most| most.get() > MOST_MILESTONES_ALLOWED)
        {
            return Err(format!(
                "milestones.most is at most {MOST_MILESTONES_ALLOWED}; lower it"
            ));
        }
        globs(&milestones.titles)
            .map(|_| ())
            .map_err(|error| format!("milestones.titles: {error}"))
    }
}

#[cfg(test)]
mod tests;
