//! GitLab's merge requests.

use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;

use super::{ForgePull, PullAction};
use crate::forge::Repository;
use crate::wire::read;

/// What GitLab's merge request event is read as.
pub(super) const HOOKED: &str = "GitLab's merge request event";
/// What GitLab's listing of merge requests is read as.
pub(super) const LISTED: &str = "GitLab's merge requests";
/// The name GitLab gives its merge request webhook's deliveries.
pub(super) const HOOK: &str = "Merge Request Hook";
/// What a project's or a group's access token bot's username holds.
const TOKEN_BOT: &str = "_bot_";
/// What a service's bot's username ends with.
const BOT_ENDS: [&str; 2] = ["-bot", "[bot]"];

/// Whether `username` is a bot's.
fn bot(username: &str) -> bool {
    username.contains(TOKEN_BOT) || BOT_ENDS.iter().any(|end| username.ends_with(end))
}

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

/// A merge request, as GitLab's API lists it.
#[derive(Deserialize)]
struct Listed {
    iid: u64,
    title: String,
    web_url: String,
    state: String,
    author: User,
    #[serde(default)]
    draft: bool,
    merged_by: Option<User>,
    merged_at: Option<Timestamp>,
    closed_at: Option<Timestamp>,
    created_at: Timestamp,
    updated_at: Option<Timestamp>,
    source_branch: Option<String>,
    target_branch: Option<String>,
    #[serde(default)]
    labels: Vec<String>,
}

impl Listed {
    /// What its state says of it: its opening where it is open, else how
    /// it closed; nothing for a locked one.
    fn action(&self) -> Option<PullAction> {
        match self.state.as_str() {
            "opened" => Some(PullAction::Opened),
            "merged" => Some(PullAction::Merged),
            "closed" => Some(PullAction::Closed),
            _ => None,
        }
    }

    /// The merge request, as it is once it did `action`.
    fn read(self, action: PullAction) -> ForgePull {
        let occurred = match action {
            PullAction::Merged => self.merged_at,
            PullAction::Closed => self.closed_at,
            _ => Some(self.created_at),
        }
        .or(self.updated_at)
        .unwrap_or(self.created_at);
        ForgePull {
            number: self.iid,
            action,
            title: self.title,
            bot: bot(&self.author.username),
            author: Some(self.author.username),
            base: self.target_branch,
            head: self.source_branch,
            draft: self.draft,
            merged_by: self.merged_by.map(|user| user.username),
            labels: self.labels,
            url: self.web_url,
            occurred,
        }
    }
}

/// A draft's change, as GitLab's webhook names it.
#[derive(Deserialize)]
struct Drafted {
    previous: Option<bool>,
    current: Option<bool>,
}

/// What a merge request event changed.
#[derive(Deserialize, Default)]
struct Changes {
    draft: Option<Drafted>,
}

/// A merge request, as GitLab's webhook describes it.
#[derive(Deserialize)]
struct Attributes {
    iid: u64,
    title: String,
    url: String,
    action: Option<String>,
    #[serde(default)]
    draft: bool,
    source_branch: Option<String>,
    target_branch: Option<String>,
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

/// GitLab's merge request event.
#[derive(Deserialize)]
struct Hooked {
    user: User,
    project: Project,
    object_attributes: Attributes,
    #[serde(default)]
    labels: Vec<Label>,
    #[serde(default)]
    changes: Changes,
}

/// What a merge request event's action, and how it changed whether it is
/// a draft, says of it: nothing for one that neither opens, readies,
/// reopens nor closes it.
fn hooked_action(action: &str, readied: bool) -> Option<PullAction> {
    match action {
        "open" => Some(PullAction::Opened),
        "reopen" => Some(PullAction::Reopened),
        "merge" => Some(PullAction::Merged),
        "close" => Some(PullAction::Closed),
        "update" if readied => Some(PullAction::ReadyForReview),
        _ => None,
    }
}

/// What GitLab's merge request event `payload` names: the project's full
/// path, the project, and the merge request where its action is one
/// herald announces. Its author is named only where the event opens it, as
/// GitLab names only who acted.
pub(super) fn delivered(
    payload: &[u8],
) -> Result<(String, Repository, Option<ForgePull>), SourceError> {
    let hooked: Hooked = read(HOOKED, payload)?;
    let attributes = hooked.object_attributes;
    let readied = hooked
        .changes
        .draft
        .is_some_and(|drafted| drafted.previous == Some(true) && drafted.current == Some(false));
    let action = hooked_action(attributes.action.as_deref().unwrap_or_default(), readied);
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
    let actor = hooked.user.username;
    let Some(action) = action else {
        return Ok((project.path_with_namespace, repository, None));
    };
    let occurred = match action {
        PullAction::Opened => attributes.created_at.or(attributes.updated_at),
        _ => attributes.updated_at.or(attributes.created_at),
    }
    .ok_or(SourceError::Unreadable {
        what: HOOKED,
        reason: "the merge request names no time".to_owned(),
    })?;
    let opened = action == PullAction::Opened;
    let pull = ForgePull {
        number: attributes.iid,
        action,
        title: attributes.title,
        bot: opened && bot(&actor),
        author: opened.then(|| actor.clone()),
        base: attributes.target_branch,
        head: attributes.source_branch,
        draft: attributes.draft,
        merged_by: (action == PullAction::Merged).then_some(actor),
        labels: hooked.labels.into_iter().map(|label| label.title).collect(),
        url: attributes.url,
        occurred,
    };
    Ok((project.path_with_namespace, repository, Some(pull)))
}

/// The merge requests in GitLab's listing `body`, parsing no more than
/// `limit` bytes, each as its state says.
pub(super) fn listed(limit: usize, body: &[u8]) -> Result<Vec<ForgePull>, SourceError> {
    let listed: Vec<Listed> = herald_wire::read(LISTED, limit, body)?;
    Ok(listed
        .into_iter()
        .filter_map(|merge| merge.action().map(|action| merge.read(action)))
        .collect())
}

/// The path, under a project, of its newest `page` merge requests, the
/// most recently updated first.
pub(super) fn listing_path(page: usize) -> String {
    format!("/merge_requests?state=all&order_by=updated_at&sort=desc&per_page={page}")
}
