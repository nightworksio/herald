//! The keys that choose a forge source's pull requests.

use herald_core::{Glob, Signal};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{SourceOptions, globs};

/// What a pull request did that is announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PullAction {
    /// It was opened, or, where drafts are announced, opened as a draft.
    Opened,
    /// A draft was marked ready for review.
    ReadyForReview,
    /// It was opened again after it closed.
    Reopened,
    /// It was merged.
    Merged,
    /// It was closed without being merged.
    Closed,
}

impl PullAction {
    /// Its name, as `pulls.actions` writes it and a pull request's
    /// `action` attribute holds it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Opened => "opened",
            Self::ReadyForReview => "ready_for_review",
            Self::Reopened => "reopened",
            Self::Merged => "merged",
            Self::Closed => "closed",
        }
    }
}

/// Which pull requests a source announces, where its signals take
/// "pull-request".
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Pulls {
    /// What a pull request did that is announced: `opened`,
    /// `ready_for_review`, `reopened`, `merged`, `closed`; `["opened",
    /// "ready_for_review", "merged"]` where absent.
    pub actions: Option<Vec<PullAction>>,
    /// Whether a draft's opening is announced; false where absent, so a
    /// draft is announced once it is ready for review.
    pub drafts: Option<bool>,
    /// The labels, as globs as `include` writes them, any one of which a
    /// pull request announced carries; every pull request where empty.
    #[serde(default)]
    pub labels: Vec<String>,
    /// The authors, as globs, whose pull requests are announced, such as
    /// `dependabot[bot]`; every author where empty.
    #[serde(default)]
    pub authors: Vec<String>,
}

/// The actions announced where `pulls.actions` is absent.
pub const PULL_ACTIONS: [PullAction; 3] = [
    PullAction::Opened,
    PullAction::ReadyForReview,
    PullAction::Merged,
];

/// Which pull requests a source announces, read from its `pulls`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullChoice {
    pub(crate) actions: Vec<PullAction>,
    pub(crate) drafts: bool,
    pub(crate) labels: Vec<Glob>,
    pub(crate) authors: Vec<Glob>,
}

impl PullChoice {
    /// Whether it announces a pull request by `author`, where its forge
    /// names one, carrying `labels`, a draft where `draft` says so, that
    /// did `action`.
    pub(crate) fn takes(
        &self,
        (action, draft): (PullAction, bool),
        (author, labels): (Option<&str>, &[String]),
    ) -> bool {
        self.actions.contains(&action)
            && (self.drafts || !draft || action != PullAction::Opened)
            && (self.authors.is_empty()
                || author
                    .is_some_and(|author| self.authors.iter().any(|glob| glob.matches(author))))
            && (self.labels.is_empty()
                || labels
                    .iter()
                    .any(|label| self.labels.iter().any(|glob| glob.matches(label))))
    }
}

impl SourceOptions {
    /// Which pull requests the source announces.
    pub(crate) fn pull_choice(&self) -> PullChoice {
        let pulls = self.pulls.clone().unwrap_or_default();
        PullChoice {
            actions: pulls.actions.unwrap_or_else(|| PULL_ACTIONS.to_vec()),
            drafts: pulls.drafts.unwrap_or_default(),
            labels: globs(&pulls.labels).unwrap_or_default(),
            authors: globs(&pulls.authors).unwrap_or_default(),
        }
    }

    /// Whether `pulls` is sound, and given only where some repository the
    /// source takes announces pull requests.
    pub(super) fn pulls_check(&self) -> Result<(), String> {
        let Some(pulls) = &self.pulls else {
            return Ok(());
        };
        if !self.may_take(Signal::PullRequest) {
            return Err(
                "pulls chooses pull requests, which no repository the source takes announces; add \"pull-request\" to signals, or remove pulls".to_owned(),
            );
        }
        if pulls.actions.as_ref().is_some_and(Vec::is_empty) {
            return Err(
                "pulls.actions names nothing, so no pull request would be announced; name one, or remove pulls.actions".to_owned(),
            );
        }
        for (key, written) in [("labels", &pulls.labels), ("authors", &pulls.authors)] {
            globs(written).map_err(|error| format!("pulls.{key}: {error}"))?;
        }
        Ok(())
    }
}
