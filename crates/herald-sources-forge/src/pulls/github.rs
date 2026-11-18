//! GitHub's and Forgejo's pull requests, which both describe alike.

use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;

use super::{ForgePull, PullAction};
use crate::forge::Repository;
use crate::wire::read;

/// What GitHub's pull request event is read as.
pub(super) const HOOKED: &str = "GitHub's pull request event";
/// What GitHub's listing of pull requests is read as.
pub(super) const LISTED: &str = "GitHub's pull requests";
/// What Forgejo's pull request event is read as.
pub(super) const FORGEJO_HOOKED: &str = "Forgejo's pull request event";
/// What Forgejo's listing of pull requests is read as.
pub(super) const FORGEJO_LISTED: &str = "Forgejo's pull requests";
/// The name GitHub and Forgejo give their pull request webhook's
/// deliveries.
pub(super) const HOOK: &str = "pull_request";
/// The kind of account GitHub names a bot's or an app's.
const BOT: &str = "Bot";
/// What a bot's login ends with on Forgejo, which names no account kind.
const BOT_SUFFIX: &str = "[bot]";

/// An account, as a pull request names it.
#[derive(Deserialize)]
struct User {
    login: String,
    #[serde(rename = "type")]
    kind: Option<String>,
}

impl User {
    /// Whether it is a bot's or an app's.
    fn bot(&self) -> bool {
        self.kind.as_deref() == Some(BOT) || self.login.ends_with(BOT_SUFFIX)
    }
}

/// A branch a pull request names.
#[derive(Deserialize)]
struct Branch {
    #[serde(rename = "ref")]
    name: String,
}

/// A label a pull request carries.
#[derive(Deserialize)]
struct Label {
    name: String,
}

/// A pull request, as GitHub and Forgejo describe it.
#[derive(Deserialize)]
struct Pull {
    number: u64,
    title: String,
    html_url: String,
    state: String,
    user: User,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    merged: bool,
    merged_at: Option<Timestamp>,
    merged_by: Option<User>,
    closed_at: Option<Timestamp>,
    created_at: Timestamp,
    updated_at: Option<Timestamp>,
    base: Option<Branch>,
    head: Option<Branch>,
    #[serde(default)]
    labels: Vec<Label>,
}

impl Pull {
    /// Whether it was merged.
    const fn merged(&self) -> bool {
        self.merged || self.merged_at.is_some()
    }

    /// The pull request, as it is once it did `action`.
    fn read(self, action: PullAction) -> ForgePull {
        let occurred = match action {
            PullAction::Opened => Some(self.created_at),
            PullAction::Merged => self.merged_at,
            PullAction::Closed => self.closed_at,
            PullAction::ReadyForReview | PullAction::Reopened => self.updated_at,
        }
        .or(self.updated_at)
        .unwrap_or(self.created_at);
        ForgePull {
            number: self.number,
            action,
            title: self.title,
            bot: self.user.bot(),
            author: Some(self.user.login),
            base: self.base.map(|branch| branch.name),
            head: self.head.map(|branch| branch.name),
            draft: self.draft,
            merged_by: self.merged_by.map(|user| user.login),
            labels: self.labels.into_iter().map(|label| label.name).collect(),
            url: self.html_url,
            occurred,
        }
    }

    /// What its state, as a listing names it, says of it: its opening
    /// where it is open, else how it closed.
    fn listed(&self) -> PullAction {
        match (self.state.as_str(), self.merged()) {
            ("open", _) => PullAction::Opened,
            (_, true) => PullAction::Merged,
            (_, false) => PullAction::Closed,
        }
    }
}

/// What a pull request event's action says of it: nothing for one that
/// neither opens, readies, reopens nor closes it.
fn hooked_action(action: &str, merged: bool) -> Option<PullAction> {
    match (action, merged) {
        ("opened", _) => Some(PullAction::Opened),
        ("ready_for_review", _) => Some(PullAction::ReadyForReview),
        ("reopened", _) => Some(PullAction::Reopened),
        ("closed", true) => Some(PullAction::Merged),
        ("closed", false) => Some(PullAction::Closed),
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

/// GitHub's or Forgejo's pull request event.
#[derive(Deserialize)]
struct Hooked {
    action: String,
    pull_request: Pull,
    repository: Named,
}

/// What the pull request event `payload`, read as `what`, names: the
/// repository's full path, the repository, and the pull request where its
/// action is one herald announces.
pub(super) fn delivered(
    what: &'static str,
    payload: &[u8],
) -> Result<(String, Repository, Option<ForgePull>), SourceError> {
    let hooked: Hooked = read(what, payload)?;
    let action = hooked_action(&hooked.action, hooked.pull_request.merged());
    let repository = hooked.repository;
    Ok((
        repository.full_name,
        Repository {
            id: repository.id,
            name: repository.name,
            page: repository.html_url,
        },
        action.map(|action| hooked.pull_request.read(action)),
    ))
}

/// The pull requests in a listing `body`, read as `what`, parsing no more
/// than `limit` bytes, each as its state says.
pub(super) fn listed(
    what: &'static str,
    limit: usize,
    body: &[u8],
) -> Result<Vec<ForgePull>, SourceError> {
    let pulls: Vec<Pull> = herald_wire::read(what, limit, body)?;
    Ok(pulls
        .into_iter()
        .map(|pull| {
            let action = pull.listed();
            pull.read(action)
        })
        .collect())
}

/// The path, under a repository, of the newest `page` pull requests on
/// GitHub, the most recently updated first.
pub(super) fn listing_path(page: usize) -> String {
    format!("/pulls?state=all&sort=updated&direction=desc&per_page={page}")
}

/// The path, under a repository, of the newest `page` pull requests on
/// Forgejo, the most recently updated first.
pub(super) fn forgejo_listing_path(page: usize) -> String {
    format!("/pulls?state=all&sort=recentupdate&limit={page}")
}
