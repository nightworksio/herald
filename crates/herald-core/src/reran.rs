//! The record herald keeps of each re-run an operator applies, written
//! before the call, so herald re-runs a job of a run on one head at most
//! once (ADR-0068).

use std::fmt;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// The job a re-run is of: its forge's API, its repository's full path,
/// its run's id and its own on the forge, and the head commit it ran on.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RunKey {
    /// The base URL of the forge's API.
    pub api: String,
    /// The repository's full path.
    pub repository: String,
    /// The run's id on the forge.
    pub run: String,
    /// The job's id on the forge: the one job re-run.
    pub job: String,
    /// The head commit it ran on.
    pub head: String,
}

impl fmt::Display for RunKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}/{}/{}/{}",
            self.api, self.repository, self.run, self.job, self.head
        )
    }
}

/// The least a signed-in operator must be allowed to do on a repository
/// for herald to re-run a job of it on their word.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    /// Push to it: GitHub's write role or above, GitLab's Developer role or
    /// above.
    #[default]
    Write,
    /// Maintain it: GitHub's maintain role or above, GitLab's Maintainer
    /// role or above.
    Maintain,
    /// Administer it: GitHub's admin role, GitLab's Owner role.
    Admin,
}

impl Permission {
    /// The permission as a refusal names it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Maintain => "maintain",
            Self::Admin => "admin",
        }
    }
}

/// What became of a re-run herald asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "outcome")]
pub enum Reran {
    /// Recorded before the call, with no answer recorded since: the call
    /// may or may not have reached the forge.
    Calling,
    /// The forge said the job is re-run, or reading the job back found it
    /// re-run.
    Rerun,
    /// The forge refused the call outright with this status, so nothing
    /// was re-run.
    Refused {
        /// The status it answered with.
        status: u16,
    },
    /// The answer was lost and reading the job back found it not re-run:
    /// herald never asks again, as the call may still reach the forge.
    Unsettled,
    /// Recorded, but the call was never sent: the run's request budget
    /// ended first, or the write App's token could not be minted for it.
    Unsent,
}

impl Reran {
    /// What it says, in plain words.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::Calling => {
                "the call may or may not have reached the forge, so herald never asks for it again; re-run it by hand if it never started"
            }
            Self::Rerun => "the forge re-runs the job",
            Self::Refused { .. } => "the forge refused the call, so nothing was re-run",
            Self::Unsettled => {
                "the forge's answer was lost and the job does not read as re-run yet, so herald never asks for it again; re-run it by hand if it never starts"
            }
            Self::Unsent => {
                "the call was not sent, as the run's request budget ended first or the write App's token could not be minted for it"
            }
        }
    }
}

/// The record of one re-run: who asked, through which credential, when,
/// and what became of it; never a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RerunRecord {
    /// The job, its run and its head.
    pub key: RunKey,
    /// The login of the operator who signed in and applied it.
    pub operator: String,
    /// The name of the secret or GitHub App the call was made with.
    pub credential: String,
    /// When it was recorded, before the call.
    pub at: Timestamp,
    /// How many times the job or its run had been re-run when it was read
    /// right before the call.
    pub reruns: u64,
    /// What became of it.
    pub outcome: Reran,
}

impl RerunRecord {
    /// Whether a later apply may ask for the re-run again: only where the
    /// forge refused the call outright or it was never sent, so nothing was
    /// re-run.
    #[must_use]
    pub const fn retried(&self) -> bool {
        matches!(self.outcome, Reran::Refused { .. } | Reran::Unsent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(outcome: Reran) -> RerunRecord {
        RerunRecord {
            key: RunKey {
                api: "https://api.github.com".to_owned(),
                repository: "o/r".to_owned(),
                run: "7".to_owned(),
                job: "70".to_owned(),
                head: "abc".to_owned(),
            },
            operator: "octocat".to_owned(),
            credential: "HERALD_RERUN".to_owned(),
            at: Timestamp::UNIX_EPOCH,
            reruns: 0,
            outcome,
        }
    }

    #[test]
    fn only_a_refused_re_run_may_be_asked_for_again() {
        assert!(action(Reran::Refused { status: 403 }).retried());
        assert!(action(Reran::Unsent).retried());
        for outcome in [Reran::Calling, Reran::Rerun, Reran::Unsettled] {
            assert!(!action(outcome).retried(), "{outcome:?}");
        }
    }

    #[test]
    fn a_permission_is_named_and_ordered_from_write_to_admin() {
        assert_eq!(
            [Permission::Write, Permission::Maintain, Permission::Admin].map(Permission::name),
            ["write", "maintain", "admin"]
        );
        assert!(
            Permission::Write < Permission::Maintain && Permission::Maintain < Permission::Admin
        );
        assert_eq!(Permission::default(), Permission::Write);
        assert!(Reran::Rerun.said().contains("re-runs the job"));
        assert!(Reran::Calling.said().contains("may or may not"));
        assert!(Reran::Refused { status: 403 }.said().contains("refused"));
        assert!(Reran::Unsettled.said().contains("answer was lost"));
        assert!(Reran::Unsent.said().contains("budget ended"));
    }

    #[test]
    fn a_job_is_keyed_by_its_api_repository_run_job_and_head() {
        assert_eq!(
            action(Reran::Calling).key.to_string(),
            "https://api.github.com/o/r/7/70/abc"
        );
        let written = serde_json::to_value(action(Reran::Refused { status: 403 })).unwrap();
        assert_eq!(
            written["outcome"],
            serde_json::json!({"outcome": "refused", "status": 403})
        );
    }
}
