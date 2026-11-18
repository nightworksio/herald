//! GitLab's issues.

use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;

use super::{ForgeIssue, IssueAction};
use crate::forge::Repository;
use crate::wire::read;

/// What GitLab's issue event is read as.
const HOOKED: &str = super::HOOKED.gitlab;
/// What GitLab's listing of issues is read as.
const LISTED: &str = super::LISTED.gitlab;
/// The name GitLab gives its issue webhook's deliveries.
pub(super) const HOOK: &str = "Issue Hook";
/// The name GitLab gives its issue webhook's deliveries of a confidential
/// issue, each of which is confidential whatever its payload says.
pub(super) const CONFIDENTIAL_HOOK: &str = "Confidential Issue Hook";

/// An account, as GitLab names it.
#[derive(Deserialize)]
struct User {
    username: String,
}

/// A label, as GitLab's webhook names it.
#[derive(Deserialize)]
struct Label {
    title: String,
}

/// An issue, as GitLab's API lists it.
#[derive(Deserialize)]
struct Listed {
    iid: u64,
    title: String,
    web_url: String,
    state: String,
    author: User,
    closed_at: Option<Timestamp>,
    created_at: Timestamp,
    updated_at: Option<Timestamp>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    confidential: bool,
}

/// An issue, as GitLab's webhook describes it.
#[derive(Deserialize)]
struct Attributes {
    iid: u64,
    title: String,
    url: String,
    action: Option<String>,
    confidential: Option<bool>,
    #[serde(default, deserialize_with = "crate::gitlab::when")]
    created_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "crate::gitlab::when")]
    updated_at: Option<Timestamp>,
}

/// A project, as GitLab's webhook names it.
#[derive(Deserialize)]
struct Project {
    id: u64,
    path_with_namespace: String,
    web_url: Option<String>,
}

/// GitLab's issue event.
#[derive(Deserialize)]
struct Hooked {
    user: User,
    project: Project,
    object_attributes: Attributes,
    #[serde(default)]
    labels: Vec<Label>,
}

/// What an issue event's action says of it.
fn hooked_action(action: &str) -> Option<IssueAction> {
    match action {
        "open" => Some(IssueAction::Opened),
        "close" => Some(IssueAction::Closed),
        "reopen" => Some(IssueAction::Reopened),
        _ => None,
    }
}

/// What GitLab's issue event `payload`, delivered as `name`, names: the
/// project's full path, the project, and the issue where its action is one
/// herald announces. Its author is named only where the event opens it, as
/// GitLab names only who acted, and it is confidential where its payload or
/// its delivery says so.
pub(super) fn delivered(
    name: &str,
    payload: &[u8],
) -> Result<(String, Repository, Option<ForgeIssue>), SourceError> {
    let hooked: Hooked = read(HOOKED, payload)?;
    let attributes = hooked.object_attributes;
    let project = hooked.project;
    let repository = Repository {
        id: project.id,
        name: project
            .path_with_namespace
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned(),
        page: project.web_url,
    };
    let Some(action) = hooked_action(attributes.action.as_deref().unwrap_or_default()) else {
        return Ok((project.path_with_namespace, repository, None));
    };
    let occurred = match action {
        IssueAction::Opened => attributes.created_at.or(attributes.updated_at),
        _ => attributes.updated_at.or(attributes.created_at),
    }
    .ok_or(SourceError::Unreadable {
        what: HOOKED,
        reason: "the issue names no time".to_owned(),
    })?;
    let opened = action == IssueAction::Opened;
    let actor = hooked.user.username;
    let issue = ForgeIssue {
        number: attributes.iid,
        action,
        title: attributes.title,
        bot: opened && crate::gitlab::bot(&actor),
        author: opened.then_some(actor),
        state_reason: None,
        labels: hooked.labels.into_iter().map(|label| label.title).collect(),
        confidential: attributes.confidential.unwrap_or_default() || name == CONFIDENTIAL_HOOK,
        url: attributes.url,
        occurred,
    };
    Ok((project.path_with_namespace, repository, Some(issue)))
}

/// The issues in GitLab's listing `body`, parsing no more than `limit`
/// bytes, each as its state says.
pub(super) fn listed(limit: usize, body: &[u8]) -> Result<Vec<ForgeIssue>, SourceError> {
    let listed: Vec<Listed> = herald_wire::read(LISTED, limit, body)?;
    Ok(listed
        .into_iter()
        .map(|issue| {
            let action = if issue.state == "opened" {
                IssueAction::Opened
            } else {
                IssueAction::Closed
            };
            let occurred = match action {
                IssueAction::Closed => issue.closed_at,
                _ => Some(issue.created_at),
            }
            .or(issue.updated_at)
            .unwrap_or(issue.created_at);
            ForgeIssue {
                number: issue.iid,
                action,
                title: issue.title,
                bot: crate::gitlab::bot(&issue.author.username),
                author: Some(issue.author.username),
                state_reason: None,
                labels: issue.labels,
                confidential: issue.confidential,
                url: issue.web_url,
                occurred,
            }
        })
        .collect())
}

/// The path, under a project, of its newest `page` issues, the most
/// recently updated first, its confidential issues among them only where
/// `confidential` says so.
pub(super) fn listing_path(page: usize, confidential: bool) -> String {
    let only = if confidential {
        ""
    } else {
        "&confidential=false"
    };
    format!("/issues?state=all&order_by=updated_at&sort=desc&per_page={page}{only}")
}
