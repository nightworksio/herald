//! The keys that choose a forge source's issues.

use herald_core::{Glob, Signal};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{SourceOptions, globs};
use crate::Forge;

/// What an issue did that is announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IssueAction {
    /// It was opened.
    Opened,
    /// It was closed.
    Closed,
    /// It was opened again after it closed.
    Reopened,
}

impl IssueAction {
    /// Its name, as `issues.actions` writes it and an issue's `action`
    /// attribute holds it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Opened => "opened",
            Self::Closed => "closed",
            Self::Reopened => "reopened",
        }
    }
}

/// Which issues a source announces, where its signals take "issue".
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Issues {
    /// What an issue did that is announced: `opened`, `closed`,
    /// `reopened`; every one where absent.
    pub actions: Option<Vec<IssueAction>>,
    /// The labels, as globs as `include` writes them, any one of which an
    /// issue announced carries; every issue where empty.
    #[serde(default)]
    pub labels: Vec<String>,
    /// The authors, as globs, whose issues are announced; every author
    /// where empty.
    #[serde(default)]
    pub authors: Vec<String>,
    /// Whether GitLab's confidential issues are announced, each seen as
    /// private, so only a route declared private takes it; false where
    /// absent.
    pub confidential: Option<bool>,
}

/// The actions announced where `issues.actions` is absent.
pub const ISSUE_ACTIONS: [IssueAction; 3] = [
    IssueAction::Opened,
    IssueAction::Closed,
    IssueAction::Reopened,
];

/// Which issues a source announces, read from its `issues`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IssueChoice {
    pub(crate) actions: Vec<IssueAction>,
    pub(crate) labels: Vec<Glob>,
    pub(crate) authors: Vec<Glob>,
    pub(crate) confidential: bool,
}

impl IssueChoice {
    /// Whether it announces an issue by `author`, where its forge names
    /// one, carrying `labels`, confidential where `confidential` says so,
    /// that did `action`.
    pub(crate) fn takes(
        &self,
        (action, confidential): (IssueAction, bool),
        (author, labels): (Option<&str>, &[String]),
    ) -> bool {
        self.actions.contains(&action)
            && (self.confidential || !confidential)
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
    /// Which issues the source announces.
    pub(crate) fn issue_choice(&self) -> IssueChoice {
        let issues = self.issues.clone().unwrap_or_default();
        IssueChoice {
            actions: issues.actions.unwrap_or_else(|| ISSUE_ACTIONS.to_vec()),
            labels: globs(&issues.labels).unwrap_or_default(),
            authors: globs(&issues.authors).unwrap_or_default(),
            confidential: issues.confidential.unwrap_or_default(),
        }
    }

    /// Whether `issues` is sound on `forge`, and given only where some
    /// repository the source takes announces issues.
    pub(super) fn issues_check(&self, forge: Forge) -> Result<(), String> {
        let Some(issues) = &self.issues else {
            return Ok(());
        };
        if issues.confidential.is_some() && forge != Forge::GitLab {
            return Err(format!(
                "issues.confidential chooses GitLab's confidential issues, which {} has none of; remove it",
                forge.kind()
            ));
        }
        if !self.may_take(Signal::Issue) {
            return Err(
                "issues chooses issues, which no repository the source takes announces; add \"issue\" to signals, or remove issues".to_owned(),
            );
        }
        if issues.actions.as_ref().is_some_and(Vec::is_empty) {
            return Err(
                "issues.actions names nothing, so no issue would be announced; name one, or remove issues.actions".to_owned(),
            );
        }
        for (key, written) in [("labels", &issues.labels), ("authors", &issues.authors)] {
            globs(written).map_err(|error| format!("issues.{key}: {error}"))?;
        }
        Ok(())
    }
}
