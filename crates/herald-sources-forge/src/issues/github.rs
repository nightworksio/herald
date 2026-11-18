//! GitHub's and Forgejo's issues, which both describe alike.

use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;

use super::{ForgeIssue, IssueAction};
use crate::forge::Repository;
use crate::github::{Account, Label};
use crate::wire::read;

/// The name GitHub and Forgejo give their issue webhook's deliveries.
pub(super) const HOOK: &str = "issues";

/// An issue, as GitHub and Forgejo describe it.
#[derive(Deserialize)]
struct Issue {
    number: u64,
    title: String,
    html_url: String,
    state: String,
    user: Account,
    state_reason: Option<String>,
    closed_at: Option<Timestamp>,
    created_at: Timestamp,
    updated_at: Option<Timestamp>,
    #[serde(default)]
    labels: Vec<Label>,
    pull_request: Option<serde_json::Value>,
}

impl Issue {
    /// The issue, as it is once it did `action`.
    fn read(self, action: IssueAction) -> ForgeIssue {
        let occurred = match action {
            IssueAction::Opened => Some(self.created_at),
            IssueAction::Closed => self.closed_at,
            IssueAction::Reopened => self.updated_at,
        }
        .or(self.updated_at)
        .unwrap_or(self.created_at);
        ForgeIssue {
            number: self.number,
            action,
            title: self.title,
            bot: self.user.bot(),
            author: Some(self.user.login),
            state_reason: self.state_reason.filter(|_| action == IssueAction::Closed),
            labels: self.labels.into_iter().map(|label| label.name).collect(),
            confidential: false,
            url: self.html_url,
            occurred,
        }
    }
}

/// What an issue event's action says of it.
fn hooked_action(action: &str) -> Option<IssueAction> {
    match action {
        "opened" => Some(IssueAction::Opened),
        "closed" => Some(IssueAction::Closed),
        "reopened" => Some(IssueAction::Reopened),
        _ => None,
    }
}

/// A repository, as GitHub's and Forgejo's events name it.
#[derive(Deserialize)]
struct Named {
    id: u64,
    name: String,
    full_name: String,
    html_url: Option<String>,
}

/// GitHub's or Forgejo's issue event.
#[derive(Deserialize)]
struct Hooked {
    action: String,
    issue: Issue,
    repository: Named,
}

/// What the issue event `payload`, read as `what`, names: the repository's
/// full path, the repository, and the issue where its action is one herald
/// announces.
pub(super) fn delivered(
    what: &'static str,
    payload: &[u8],
) -> Result<(String, Repository, Option<ForgeIssue>), SourceError> {
    let hooked: Hooked = read(what, payload)?;
    let action = hooked_action(&hooked.action);
    let repository = hooked.repository;
    Ok((
        repository.full_name,
        Repository {
            id: repository.id,
            name: repository.name,
            page: repository.html_url,
        },
        action.map(|action| hooked.issue.read(action)),
    ))
}

/// The issues in a listing `body`, read as `what`, parsing no more than
/// `limit` bytes, each as its state says: open as its opening, closed as
/// its closing. GitHub lists pull requests among issues; they are left
/// out.
pub(super) fn listed(
    what: &'static str,
    limit: usize,
    body: &[u8],
) -> Result<Vec<ForgeIssue>, SourceError> {
    let issues: Vec<Issue> = herald_wire::read(what, limit, body)?;
    Ok(issues
        .into_iter()
        .filter(|issue| issue.pull_request.is_none())
        .map(|issue| {
            let action = if issue.state == "open" {
                IssueAction::Opened
            } else {
                IssueAction::Closed
            };
            issue.read(action)
        })
        .collect())
}

/// The path, under a repository, of the newest `page` issues on GitHub,
/// the most recently updated first.
pub(super) fn listing_path(page: usize) -> String {
    format!("/issues?state=all&sort=updated&direction=desc&per_page={page}")
}

/// The path, under a repository, of the newest `page` issues on Forgejo,
/// the most recently updated first.
pub(super) fn forgejo_listing_path(page: usize) -> String {
    format!("/issues?state=all&type=issues&sort=recentupdate&limit={page}")
}
