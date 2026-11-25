//! The steps herald's own queue takes for one gesture, each recorded with
//! compare-and-set before the forge is called and with what came of it
//! after, and why the pull request left the queue, once it left
//! (ADR-0087).

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use super::{Left, Retries};

/// A step herald's own queue takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepKind {
    /// The branch brought up to date with the base, through the forge.
    Update,
    /// The pull request merged, its head named as the head the forge must
    /// find.
    Merge,
    /// The failed job of a check the queue's `retries` names re-run alone.
    Rerun,
}

impl StepKind {
    /// The step, in plain words.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::Update => "bringing it up to date",
            Self::Merge => "merging it",
            Self::Rerun => "re-running a failed check",
        }
    }
}

/// What became of a step herald recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stepped {
    /// Recorded before the call, with no answer recorded since: the call
    /// may or may not have reached the forge.
    Calling,
    /// The forge took the step, or reading the pull request back found it
    /// taken.
    Took,
    /// The forge could not bring the branch up to date, as it conflicts
    /// with the base.
    Conflict,
    /// The forge refused the step.
    Refused,
    /// The forge refused the step, and the pull request's head read back
    /// is no longer the one the step named.
    HeadMoved,
    /// The answer was lost, and reading the pull request back did not find
    /// the step taken.
    Unsettled,
    /// Recorded, but the call was never sent: the run's request budget
    /// ended first, or the write credential could not be made ready.
    Unsent,
}

impl Stepped {
    /// What it says, in plain words.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::Calling => {
                "the call may or may not have reached the forge, so herald never takes a step again for this gesture"
            }
            Self::Took => "the forge took it",
            Self::Conflict => "the branch conflicts with the base",
            Self::Refused => "the forge refused it",
            Self::HeadMoved => {
                "the forge refused it, as the head is no longer the one the step named"
            }
            Self::Unsettled => {
                "the forge's answer was lost and the step was not found taken, so herald never takes a step again for this gesture"
            }
            Self::Unsent => {
                "the call was not sent, as the run's request budget ended first or the write credential could not be made ready"
            }
        }
    }

    /// Why the pull request leaves the queue after a step of `kind` came
    /// to it, where it does.
    #[must_use]
    pub const fn left(self, kind: StepKind) -> Option<Left> {
        match (self, kind) {
            (Self::Conflict, _) => Some(Left::Conflict),
            (Self::Refused, StepKind::Update) => Some(Left::CannotUpdate),
            (Self::Refused, StepKind::Merge) => Some(Left::MergeRefused),
            (Self::Refused, StepKind::Rerun) => Some(Left::ChecksFailed),
            (Self::HeadMoved, _) => Some(Left::HeadMoved),
            _ => None,
        }
    }

    /// Whether the forge may or may not have taken it, so no step is ever
    /// taken again for its gesture.
    #[must_use]
    pub const fn unknown(self) -> bool {
        matches!(self, Self::Calling | Self::Unsettled)
    }
}

/// One step: its kind, the head it named, the base's head read with it,
/// the credential's name, when it was recorded, and what became of it;
/// never a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// The step.
    pub kind: StepKind,
    /// The pull request's head the step was taken on.
    pub head: String,
    /// The base branch's head when it was read right before the step.
    pub base: String,
    /// Whether the forge said, read right before the step, that the head
    /// merges into that base's head without a conflict.
    #[serde(default)]
    pub clean: bool,
    /// The pull request's head read right after an update the forge took,
    /// where the forge made and signed it with exactly `head` and `base` as
    /// its parents, in that order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made: Option<String>,
    /// The check whose failed job it re-ran, where it is a re-run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
    /// The name of the secret or GitHub App the call was made with.
    pub credential: String,
    /// When it was recorded, before the call.
    pub at: Timestamp,
    /// What became of it.
    pub outcome: Stepped,
}

/// Why and when a pull request left herald's own queue for one gesture: it
/// comes back only by a new gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Leaving {
    /// Why it left.
    pub left: Left,
    /// When herald recorded it.
    pub at: Timestamp,
}

/// What herald's own queue recorded for one gesture: its steps, oldest
/// first, and why it left, once it left.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Own {
    /// Each step recorded, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<Step>,
    /// Why it left the queue, once it left.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<Leaving>,
}

impl Own {
    /// Whether nothing was recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty() && self.left.is_none()
    }

    /// The step that holds every further one back: one the forge may or
    /// may not have taken, or an update or a merge of `kind` already
    /// recorded on `head` that was sent.
    #[must_use]
    pub fn holding(&self, kind: StepKind, head: &str) -> Option<&Step> {
        self.steps.iter().rev().find(|step| {
            step.outcome.unknown()
                || (step.kind == kind
                    && kind != StepKind::Rerun
                    && step.head == head
                    && step.outcome != Stepped::Unsent)
        })
    }

    /// How many re-runs of the check `check` on `head` were sent: the
    /// steps naming it, as a re-run alone does.
    #[must_use]
    pub fn reruns(&self, check: &str, head: &str) -> usize {
        self.steps
            .iter()
            .filter(|step| {
                step.check.as_deref() == Some(check)
                    && step.head == head
                    && step.outcome != Stepped::Unsent
            })
            .count()
    }
}

/// The failed check of `failing` whose job `retries` re-runs next on
/// `head`, as `own` records the re-runs sent: the first, where `retries`
/// names every check of `failing` and none was re-run `times` on `head`
/// yet; none where a check `retries` does not name failed, or one was
/// re-run as often as it lets.
#[must_use]
pub fn rerun<'c>(
    retries: Option<&Retries>,
    failing: &[&'c str],
    own: Option<&Own>,
    head: &str,
) -> Option<&'c str> {
    let retries = retries?;
    let times = usize::from(retries.times.get());
    let rerunnable = |check: &&str| {
        retries.checks.iter().any(|named| named == check)
            && own.map_or(0, |own| own.reruns(check, head)) < times
    };
    if failing.iter().all(rerunnable) {
        failing.first().copied()
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
