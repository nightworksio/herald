//! GitHub's and Forgejo's deliveries, which both describe alike.

use herald_core::{
    Described, Folded, Left, Lifecycle, Mergeable, Outcome, QueueEntry, QueueState, Verdict,
    Visibility,
};
use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;

use super::{Delivery, Read, Said, concluded, keyed};
use crate::github::{Account, Label};
use crate::owner::{flagged, named};
use crate::queue::TRUSTED;
use crate::queue::merging::{dequeued, grouped};
use crate::wire::read;

#[derive(Deserialize)]
struct Repository {
    id: u64,
    full_name: String,
    html_url: String,
    visibility: Option<String>,
    private: Option<bool>,
    internal: Option<bool>,
}

impl Repository {
    /// Who may see it, where the delivery says.
    fn visibility(&self) -> Option<Visibility> {
        self.visibility.as_deref().map(named).or_else(|| {
            self.private
                .map(|private| flagged(Some(private), self.internal))
        })
    }
}

#[derive(Deserialize)]
struct Head {
    sha: String,
}

#[derive(Deserialize)]
struct Team {
    slug: String,
}

#[derive(Deserialize)]
struct Pull {
    number: u64,
    title: String,
    html_url: String,
    state: String,
    user: Option<Account>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    merged: bool,
    merged_at: Option<Timestamp>,
    created_at: Timestamp,
    updated_at: Option<Timestamp>,
    head: Option<Head>,
    labels: Option<Vec<Label>>,
    requested_reviewers: Option<Vec<Account>>,
    requested_teams: Option<Vec<Team>>,
    mergeable: Option<bool>,
}

#[derive(Deserialize)]
struct Review {
    user: Option<Account>,
    state: Option<String>,
    submitted_at: Option<Timestamp>,
    author_association: Option<String>,
}

#[derive(Deserialize)]
struct Issue {
    number: u64,
    pull_request: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct Comment {
    created_at: Timestamp,
    updated_at: Option<Timestamp>,
}

#[derive(Deserialize)]
struct CheckRun {
    name: String,
    head_sha: String,
    status: String,
    conclusion: Option<String>,
    started_at: Option<Timestamp>,
    completed_at: Option<Timestamp>,
}

#[derive(Deserialize)]
struct MergeGroup {
    head_ref: String,
}

/// One delivery of an event kept mode folds, each part the event does not
/// carry absent.
#[derive(Deserialize)]
struct Delivered {
    action: Option<String>,
    reason: Option<String>,
    merge_group: Option<MergeGroup>,
    repository: Repository,
    pull_request: Option<Pull>,
    review: Option<Review>,
    sender: Option<Account>,
    issue: Option<Issue>,
    comment: Option<Comment>,
    check_run: Option<CheckRun>,
    sha: Option<String>,
    state: Option<String>,
    context: Option<String>,
    updated_at: Option<Timestamp>,
}

impl Pull {
    /// When anyone last acted on it.
    fn updated(&self) -> Timestamp {
        self.updated_at.unwrap_or(self.created_at)
    }

    /// It, as the delivery describes it, shown as `project` of the
    /// repository at `path` seen by `visibility`, its merge queue entry as
    /// `queue` says.
    fn described(
        self,
        (project, path, visibility): (&str, &str, Option<Visibility>),
        queue: Option<QueueEntry>,
    ) -> Described {
        let updated = self.updated();
        let lifecycle = match (self.state.as_str(), self.merged || self.merged_at.is_some()) {
            ("open", _) => Lifecycle::Open,
            (_, true) => Lifecycle::Merged,
            (_, false) => Lifecycle::Closed,
        };
        let owner = path.split_once('/').map_or(path, |(owner, _)| owner);
        let reviewers = self.requested_reviewers.map(|reviewers| {
            reviewers
                .into_iter()
                .map(|reviewer| reviewer.login)
                .chain(
                    self.requested_teams
                        .into_iter()
                        .flatten()
                        .map(|team| format!("{owner}/{}", team.slug)),
                )
                .collect()
        });
        Described {
            number: self.number,
            project: project.to_owned(),
            repository: path.to_owned(),
            visibility,
            title: self.title,
            url: self.html_url,
            author: self.user.map(|user| {
                let bot = user.bot();
                (user.login, bot)
            }),
            draft: self.draft,
            labels: self
                .labels
                .map(|labels| labels.into_iter().map(|label| label.name).collect()),
            reviewers,
            head: self.head.map(|head| head.sha),
            mergeable: self.mergeable.map(|clean| {
                if clean {
                    Mergeable::Clean
                } else {
                    Mergeable::Conflict
                }
            }),
            queue,
            forge: None,
            opened: self.created_at,
            updated,
            lifecycle,
        }
    }
}

/// What a GitHub review's state says: a verdict, its dismissal, or
/// nothing where it only comments.
fn verdict(state: &str) -> Option<Said> {
    match state.to_ascii_lowercase().as_str() {
        "approved" => Some(Said::Gave(Verdict::Approved)),
        "changes_requested" => Some(Said::Gave(Verdict::ChangesRequested)),
        "dismissed" => Some(Said::Withdrew),
        _ => None,
    }
}

/// What GitHub's or Forgejo's delivery `payload` of `delivery`, read as
/// `what`, says of the pull requests of its repository, shown as
/// `project`.
///
/// # Errors
///
/// When the payload is not the event it is named.
pub(super) fn folded(
    what: &'static str,
    delivery: Delivery,
    payload: &[u8],
    project: &str,
) -> Result<Read, SourceError> {
    let delivered: Delivered = read(what, payload)?;
    let repository = delivered.repository;
    let key = keyed(what, &repository.html_url, repository.id)?;
    let visibility = repository.visibility();
    let path = repository.full_name;
    let named = (project, path.as_str(), visibility);
    let folded = match (delivery, delivered.pull_request) {
        (Delivery::Pull, Some(pull)) => {
            let at = pull.updated();
            let queue = match delivered.action.as_deref() {
                Some("enqueued") => Some(QueueEntry::entered(Some(at))),
                Some("dequeued") => {
                    let reason = dequeued(delivered.reason.as_deref().unwrap_or_default());
                    Some(QueueEntry::leaving(None, Left { at, reason }))
                }
                _ => None,
            };
            vec![Folded::Pull(Box::new(pull.described(named, queue)))]
        }
        (Delivery::Review(approved), Some(pull)) => {
            let acted = (pull.number, pull.updated());
            let review = reviewed(delivered.review, delivered.sender, approved, acted);
            let mut folded = vec![Folded::Pull(Box::new(pull.described(named, None)))];
            folded.extend(review);
            folded
        }
        (Delivery::Remark, Some(pull)) => vec![Folded::Activity {
            number: pull.number,
            at: delivered
                .comment
                .map_or_else(|| pull.updated(), |comment| comment.at()),
        }],
        (Delivery::Comment, _) => delivered
            .issue
            .zip(delivered.comment)
            .filter(|(issue, _)| issue.pull_request.is_some())
            .map(|(issue, comment)| Folded::Activity {
                number: issue.number,
                at: comment.at(),
            })
            .into_iter()
            .collect(),
        (Delivery::Check, _) => delivered
            .check_run
            .and_then(CheckRun::folded)
            .into_iter()
            .collect(),
        (Delivery::Status, _) => {
            let status = (
                delivered.sha,
                delivered.state,
                delivered.context,
                delivered.updated_at,
            );
            match status {
                (Some(head), Some(state), Some(name), Some(at)) => vec![Folded::Check {
                    outcome: if state == "pending" {
                        Outcome::Pending
                    } else {
                        concluded(Some(&state))
                    },
                    head,
                    name,
                    at,
                }],
                _ => Vec::new(),
            }
        }
        (Delivery::Repository, _) => visibility.map(Folded::Visibility).into_iter().collect(),
        (Delivery::MergeGroup, _) => {
            let state = match delivered.action.as_deref() {
                Some("checks_requested") => Some(QueueState::Checking),
                Some("destroyed") if delivered.reason.as_deref() != Some("merged") => {
                    Some(QueueState::Queued)
                }
                _ => None,
            };
            delivered
                .merge_group
                .and_then(|group| grouped(&group.head_ref))
                .zip(state)
                .map(|(number, state)| Folded::Queue { number, state })
                .into_iter()
                .collect()
        }
        (Delivery::Pull | Delivery::Review(_) | Delivery::Remark, None) => Vec::new(),
    };
    Ok((path, key, folded))
}

impl Comment {
    /// When it was last written.
    fn at(&self) -> Timestamp {
        self.updated_at.unwrap_or(self.created_at)
    }
}

impl CheckRun {
    /// What it says of its head, where it says when.
    fn folded(self) -> Option<Folded> {
        let at = self.completed_at.or(self.started_at)?;
        let conclusion = (self.status == "completed").then(|| self.conclusion.unwrap_or_default());
        Some(Folded::Check {
            head: self.head_sha,
            name: self.name,
            outcome: concluded(conclusion.as_deref()),
            at,
        })
    }
}

/// What a review delivery says of pull request `number` at `at`: GitHub's
/// review by its reviewer, deciding where the reviewer's association does;
/// Forgejo's approval or rejection by its sender, whose weight the
/// delivery does not say.
fn reviewed(
    review: Option<Review>,
    sender: Option<Account>,
    approved: Option<bool>,
    (number, at): (u64, Timestamp),
) -> Option<Folded> {
    if let Some(approved) = approved {
        let verdict = if approved {
            Verdict::Approved
        } else {
            Verdict::ChangesRequested
        };
        return sender.map(|sender| Folded::Review {
            number,
            reviewer: sender.login,
            verdict: Some(verdict),
            decides: None,
            at,
        });
    }
    let review = review?;
    let verdict = verdict(review.state.as_deref()?)?;
    Some(Folded::Review {
        number,
        reviewer: review.user?.login,
        verdict: verdict.verdict(),
        decides: review
            .author_association
            .as_deref()
            .map(|association| TRUSTED.contains(&association)),
        at: review.submitted_at.unwrap_or(at),
    })
}
