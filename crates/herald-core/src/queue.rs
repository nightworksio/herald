//! A maintainer's queue: the open pull requests a view shows, whose turn
//! each is, and how long it has waited.

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::members::Visibility;

mod bump;
mod view;

pub use bump::{Bump, BumpPattern, NotABumpPattern, SHIPPED_BUMPS, bumped};
pub use view::{Batch, Board, Bots, Deciding, GroupBy, Grouped, Row, View, board};

/// Whose turn an open pull request is.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Turn {
    /// It is a draft.
    Draft,
    /// It is in the forge's merge queue or train.
    Queued,
    /// Its author's: a change was asked for, a check failed, or it
    /// conflicts.
    WaitingOnAuthor,
    /// Its checks', which are still running.
    WaitingOnChecks,
    /// A reviewer's.
    WaitingOnReview,
    /// A maintainer's, to merge it: approved, green and mergeable.
    WaitingOnMerge,
    /// Nobody's that herald can tell.
    Unknown,
}

impl Turn {
    /// Every state, in the order a view lists them.
    pub const ALL: [Self; 7] = [
        Self::WaitingOnReview,
        Self::WaitingOnMerge,
        Self::WaitingOnAuthor,
        Self::WaitingOnChecks,
        Self::Queued,
        Self::Draft,
        Self::Unknown,
    ];

    /// The state as a view, a template and a page name it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Queued => "queued",
            Self::WaitingOnAuthor => "waiting-on-author",
            Self::WaitingOnChecks => "waiting-on-checks",
            Self::WaitingOnReview => "waiting-on-review",
            Self::WaitingOnMerge => "waiting-on-merge",
            Self::Unknown => "unknown",
        }
    }

    /// The state named `name`.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.name() == name)
    }
}

/// What a pull request's reviews say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Review {
    /// Nobody reviewed it or was asked to.
    #[default]
    Unasked,
    /// A review was asked for and not yet given.
    Requested,
    /// It was approved.
    Approved,
    /// A change was asked for.
    ChangesRequested,
}

/// What a pull request's head commit's checks say.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Checks {
    /// It has none.
    #[default]
    Absent,
    /// Some are still running and none failed.
    Pending,
    /// Every one passed.
    Passed,
    /// These failed, by name.
    Failed(Vec<String>),
}

/// Whether a pull request can merge into its base as it stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mergeable {
    /// The forge has not said.
    #[default]
    Unknown,
    /// It merges cleanly.
    Clean,
    /// It conflicts with its base.
    Conflict,
}

/// An open pull request, as a view reads it from any forge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pull {
    /// The project it is announced as.
    pub project: String,
    /// Its repository's full path.
    pub repository: String,
    /// Who may see its repository.
    pub visibility: Visibility,
    /// Its number in the repository.
    pub number: u64,
    /// Its title.
    pub title: String,
    /// Its page.
    pub url: String,
    /// Who opened it, where the forge names them.
    pub author: Option<String>,
    /// Whether its author is a bot or an app.
    pub bot: bool,
    /// Whether it is a draft.
    pub draft: bool,
    /// Its labels.
    pub labels: Vec<String>,
    /// When it was opened.
    pub opened: Timestamp,
    /// When anyone last acted on it.
    pub updated: Timestamp,
    /// What its reviews say.
    pub review: Review,
    /// What its head's checks say.
    pub checks: Checks,
    /// Whether it merges cleanly.
    pub mergeable: Mergeable,
    /// Whether it is in the forge's merge queue or train.
    pub queued: bool,
    /// The state the forge's own status maps to, with the status as the
    /// forge names it, where the forge gives one.
    pub forge: Option<(Turn, String)>,
}

/// The rules herald tells whose turn a pull request is by, each of which
/// a view may switch off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Rules {
    /// A draft waits on nobody but its author's readiness.
    pub draft: bool,
    /// One in the merge queue waits on the queue.
    pub queued: bool,
    /// One that conflicts waits on its author.
    pub conflict: bool,
    /// One a reviewer asked to change waits on its author.
    pub changes: bool,
    /// One whose check failed waits on its author.
    pub failed: bool,
    /// One whose checks are running waits on them.
    pub pending: bool,
    /// One waits on review until it is approved.
    pub approval: bool,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            draft: true,
            queued: true,
            conflict: true,
            changes: true,
            failed: true,
            pending: true,
            approval: true,
        }
    }
}

/// Why a pull request is in its state, as a row names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// It is a draft.
    Draft,
    /// It is in the merge queue.
    Queued,
    /// It conflicts with its base.
    Conflict,
    /// A reviewer asked for a change.
    ChangesRequested,
    /// These checks failed.
    CheckFailed(Vec<String>),
    /// Its checks are running.
    ChecksPending,
    /// A review was asked for.
    ReviewRequested,
    /// Nobody approved it yet.
    Unapproved,
    /// It is approved, green and mergeable.
    Ready,
    /// The forge's own status, as the forge names it.
    Forge(String),
    /// The label that put it there.
    Label(String),
    /// Nothing tells.
    Untold,
}

impl Reason {
    /// The reason as a row writes it, as in `check-failed: test`.
    #[must_use]
    pub fn written(&self) -> String {
        match self {
            Self::Draft => "draft".to_owned(),
            Self::Queued => "queued".to_owned(),
            Self::Conflict => "conflict".to_owned(),
            Self::ChangesRequested => "changes-requested".to_owned(),
            Self::CheckFailed(names) => format!("check-failed: {}", names.join(", ")),
            Self::ChecksPending => "checks-pending".to_owned(),
            Self::ReviewRequested => "review-requested".to_owned(),
            Self::Unapproved => "unapproved".to_owned(),
            Self::Ready => "ready".to_owned(),
            Self::Forge(status) => format!("forge: {status}"),
            Self::Label(label) => format!("label: {label}"),
            Self::Untold => "untold".to_owned(),
        }
    }
}

impl Pull {
    /// Whose turn it is by herald's own `rules`, and why: the first rule
    /// that holds, in the order [`Rules`] lists them.
    #[must_use]
    pub fn waiting(&self, rules: &Rules) -> (Turn, Reason) {
        if rules.draft && self.draft {
            return (Turn::Draft, Reason::Draft);
        }
        if rules.queued && self.queued {
            return (Turn::Queued, Reason::Queued);
        }
        if rules.conflict && self.mergeable == Mergeable::Conflict {
            return (Turn::WaitingOnAuthor, Reason::Conflict);
        }
        if rules.changes && self.review == Review::ChangesRequested {
            return (Turn::WaitingOnAuthor, Reason::ChangesRequested);
        }
        if let (true, Checks::Failed(names)) = (rules.failed, &self.checks) {
            return (Turn::WaitingOnAuthor, Reason::CheckFailed(names.clone()));
        }
        if rules.pending && self.checks == Checks::Pending {
            return (Turn::WaitingOnChecks, Reason::ChecksPending);
        }
        if rules.approval && self.review != Review::Approved {
            let reason = if self.review == Review::Requested {
                Reason::ReviewRequested
            } else {
                Reason::Unapproved
            };
            return (Turn::WaitingOnReview, reason);
        }
        (Turn::WaitingOnMerge, Reason::Ready)
    }

    /// Whose turn it is by the forge's own status, where the forge gives
    /// one.
    #[must_use]
    pub fn forge_waiting(&self) -> (Turn, Reason) {
        self.forge
            .as_ref()
            .map_or((Turn::Unknown, Reason::Untold), |(state, status)| {
                (*state, Reason::Forge(status.clone()))
            })
    }

    /// Whose turn it is by the first of `labels` it carries, each a label
    /// and the state it puts a pull request in, ignoring case.
    #[must_use]
    pub fn labelled_waiting(&self, labels: &[(String, Turn)]) -> (Turn, Reason) {
        labels
            .iter()
            .find(|(label, _)| {
                self.labels
                    .iter()
                    .any(|held| held.eq_ignore_ascii_case(label))
            })
            .map_or((Turn::Unknown, Reason::Untold), |(label, state)| {
                (*state, Reason::Label(label.clone()))
            })
    }

    /// Whether its checks all passed and it merges cleanly.
    #[must_use]
    pub fn green(&self) -> bool {
        self.checks == Checks::Passed && self.mergeable == Mergeable::Clean
    }

    /// Whether a check of its failed.
    #[must_use]
    pub const fn failing(&self) -> bool {
        matches!(self.checks, Checks::Failed(_))
    }
}

#[cfg(test)]
mod tests;
