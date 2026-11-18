use std::collections::BTreeMap;

use herald_core::{Checks, Review};
use herald_wire::SourceError;
use serde::Deserialize;

use super::{FAILED, PAGE};
use crate::Forge;

/// What a pull request's reviews are read as.
pub(super) const REVIEWS: &str = "a pull request's reviews";

/// What a head commit's checks are read as.
pub(super) const CHECKS: &str = "a commit's checks";

/// The state a commit status on Forgejo has while it runs.
const RUNNING: &str = "pending";

/// The state a check run on GitHub has once it ran.
const COMPLETED: &str = "completed";

/// The path, under a repository on `forge`, of pull request `number`'s
/// reviews.
pub(super) fn reviews_path(forge: Forge, number: u64) -> String {
    format!("/pulls/{number}/reviews?{}={PAGE}", forge.api().page)
}

/// The path, under a repository on `forge`, of commit `sha`'s checks.
pub(super) fn checks_path(forge: Forge, sha: &str) -> String {
    let page = forge.api().page;
    match forge {
        Forge::Forgejo => format!("/commits/{sha}/status?{page}={PAGE}"),
        Forge::GitHub | Forge::GitLab => format!("/commits/{sha}/check-runs?{page}={PAGE}"),
    }
}

#[derive(Deserialize)]
struct User {
    login: String,
}

#[derive(Deserialize)]
struct Given {
    user: Option<User>,
    state: String,
    #[serde(default)]
    dismissed: bool,
}

/// What one review leaves its reviewer's verdict as.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Approved,
    Changes,
    Dismissed,
}

impl Given {
    /// What it leaves its reviewer's verdict as, where it changes it.
    fn verdict(&self) -> Option<Verdict> {
        if self.dismissed {
            return Some(Verdict::Dismissed);
        }
        match self.state.as_str() {
            "APPROVED" => Some(Verdict::Approved),
            "CHANGES_REQUESTED" | "REQUEST_CHANGES" => Some(Verdict::Changes),
            "DISMISSED" => Some(Verdict::Dismissed),
            _ => None,
        }
    }
}

/// What the reviews `body` holds say, oldest first, each reviewer's last
/// verdict standing, parsing no more than `parse` bytes; `requested` where
/// a review is asked for and none was given.
pub(super) fn reviews(body: &[u8], parse: usize, requested: bool) -> Result<Review, SourceError> {
    let given: Vec<Given> = herald_wire::read(REVIEWS, parse, body)?;
    let mut standing: BTreeMap<String, Verdict> = BTreeMap::new();
    for review in given {
        if let (Some(user), Some(verdict)) = (&review.user, review.verdict()) {
            standing.insert(user.login.clone(), verdict);
        }
    }
    let said = |wanted| standing.values().any(|verdict| *verdict == wanted);
    Ok(if said(Verdict::Changes) {
        Review::ChangesRequested
    } else if said(Verdict::Approved) {
        Review::Approved
    } else if requested {
        Review::Requested
    } else {
        Review::Unasked
    })
}

#[derive(Deserialize)]
struct Runs {
    #[serde(default)]
    check_runs: Vec<Run>,
}

#[derive(Deserialize)]
struct Run {
    name: String,
    status: String,
    conclusion: Option<String>,
}

#[derive(Deserialize)]
struct Combined {
    #[serde(default)]
    statuses: Vec<Status>,
}

#[derive(Deserialize)]
struct Status {
    context: String,
    status: String,
}

/// Whether `state` is one a failed check ends in.
fn failed(state: &str) -> bool {
    FAILED.iter().any(|named| named.eq_ignore_ascii_case(state))
}

/// What checks say, each a name, whether it ran, and whether it failed.
fn fold(checks: impl Iterator<Item = (String, bool, bool)>) -> Checks {
    let checks: Vec<(String, bool, bool)> = checks.collect();
    if checks.is_empty() {
        return Checks::Absent;
    }
    let names: Vec<String> = checks
        .iter()
        .filter(|(_, _, failed)| *failed)
        .map(|(name, _, _)| name.clone())
        .collect();
    if !names.is_empty() {
        Checks::Failed(names)
    } else if checks.iter().all(|(_, ran, _)| *ran) {
        Checks::Passed
    } else {
        Checks::Pending
    }
}

/// What the checks `body` holds say on `forge`: GitHub's check runs, or
/// Forgejo's combined status, parsing no more than `parse` bytes.
pub(super) fn checks(forge: Forge, body: &[u8], parse: usize) -> Result<Checks, SourceError> {
    Ok(match forge {
        Forge::Forgejo => {
            let combined: Combined = herald_wire::read(CHECKS, parse, body)?;
            fold(combined.statuses.into_iter().map(|status| {
                let ran = status.status != RUNNING;
                let failed = failed(&status.status);
                (status.context, ran, failed)
            }))
        }
        Forge::GitHub | Forge::GitLab => {
            let runs: Runs = herald_wire::read(CHECKS, parse, body)?;
            fold(runs.check_runs.into_iter().map(|run| {
                let failed = run.conclusion.as_deref().is_some_and(failed);
                (run.name, run.status == COMPLETED, failed)
            }))
        }
    })
}

#[cfg(test)]
mod tests;
