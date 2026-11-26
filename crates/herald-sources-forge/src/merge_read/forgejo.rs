//! A merge queue's reads on Forgejo and Gitea, such as Codeberg.

use herald_core::Glob;
use herald_core::merging::{Check, CheckState};
use herald_kinds::{Branched, Entered, Holds, Proved, Queued, Unpolled};
use herald_ports::{DynHttp, Erased};
use jiff::Timestamp;
use serde::Deserialize;

use super::{
    APPROVED, Actor, ForgeMergeReader, MOST_MARKS, Mark, OPEN, Opened, PULL, Push, Pushes, Review,
    Reviews, Said, label_actor,
};
use crate::forge::whole;
use crate::owner::flagged;
use crate::paging::page;
use crate::roles::forgejo_role;

/// The timeline entry of a label added or removed.
const LABEL: &str = "label";

/// The timeline entry of a comment.
const COMMENT: &str = "comment";

/// The timeline entry of a push to a pull request's head.
const PULL_PUSH: &str = "pull_push";

/// What a label entry's body says where the label was added.
const ADDED: &str = "1";

/// The states of a commit status that pass.
const PASSING: [&str; 2] = ["success", "skipped"];

/// The state of a commit status still running.
const PENDING: &str = "pending";

/// The review states that neither approve nor ask for changes.
const NEUTRAL: [&str; 3] = ["COMMENT", "PENDING", "REQUEST_REVIEW"];

#[derive(Deserialize)]
struct User {
    login: String,
    id: Option<u64>,
}

impl From<User> for Actor {
    fn from(user: User) -> Self {
        Self {
            login: user.login,
            id: user.id,
        }
    }
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

#[derive(Deserialize)]
struct Repository {
    full_name: String,
}

#[derive(Deserialize)]
struct Head {
    sha: String,
    #[serde(rename = "ref")]
    branch: Option<String>,
    repo: Option<Repository>,
}

#[derive(Deserialize)]
struct Base {
    #[serde(rename = "ref")]
    branch: String,
}

#[derive(Deserialize)]
struct Pull {
    number: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    draft: bool,
    head: Head,
    base: Base,
    user: Option<User>,
    #[serde(default)]
    labels: Vec<Label>,
}

impl From<Pull> for Opened {
    fn from(pull: Pull) -> Self {
        Self {
            number: pull.number,
            title: pull.title,
            body: pull.body.unwrap_or_default(),
            url: pull.html_url,
            id: None,
            draft: pull.draft,
            head: pull.head.sha,
            author: pull.user.map(|user| user.login),
            labels: pull.labels.into_iter().map(|label| label.name).collect(),
            head_repository: pull.head.repo.map(|repo| repo.full_name),
            head_branch: pull.head.branch,
        }
    }
}

#[derive(Deserialize)]
struct Entry {
    #[serde(default)]
    id: Option<u64>,
    #[serde(rename = "type")]
    kind: String,
    user: Option<User>,
    created_at: Timestamp,
    updated_at: Option<Timestamp>,
    #[serde(default)]
    body: String,
    label: Option<Label>,
}

#[derive(Deserialize)]
struct Pushed {
    #[serde(default)]
    is_force_push: bool,
    #[serde(default)]
    commit_ids: Vec<String>,
}

#[derive(Deserialize)]
struct Collaborator {
    permission: String,
}

#[derive(Deserialize)]
struct Flags {
    private: Option<bool>,
    internal: Option<bool>,
}

#[derive(Deserialize)]
struct Commit {
    id: String,
}

#[derive(Deserialize)]
struct Branch {
    commit: Commit,
    #[serde(default)]
    enable_status_check: bool,
    #[serde(default)]
    status_check_contexts: Vec<String>,
}

#[derive(Deserialize)]
struct Merging {
    merge_base: Option<String>,
    mergeable: Option<bool>,
}

#[derive(Deserialize)]
struct Status {
    context: String,
    status: String,
}

#[derive(Deserialize)]
struct Combined {
    total_count: usize,
    #[serde(default)]
    statuses: Vec<Status>,
}

/// The open pull requests into `branch`, at most [`MOST_OPENED`] open ones
/// read.
pub(super) async fn opened(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    branch: &str,
) -> Result<Vec<Opened>, Unpolled> {
    let ask = |query: &str| reader.get(&format!("/pulls?state=open&{query}"));
    let pulls: Vec<Pull> = reader.opened(http, &ask).await?;
    Ok(pulls
        .into_iter()
        .filter(|pull| pull.base.branch == branch)
        .map(Opened::from)
        .collect())
}

/// The pull request `number`, where it is open into `branch`.
pub(super) async fn one(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    (branch, number): (&str, u64),
) -> Result<Option<Opened>, Unpolled> {
    let pull: Option<Pull> = reader
        .read(http, reader.get(&format!("/pulls/{number}"))?, PULL)
        .await?;
    Ok(pull
        .filter(|pull| pull.state == OPEN && pull.base.branch == branch)
        .map(Opened::from))
}

/// The timeline of pull request `number`, and whether it is whole.
async fn timeline(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    number: u64,
) -> Result<(Vec<Entry>, bool), Unpolled> {
    let ask = |query: &str| reader.get(&format!("/issues/{number}/timeline?{query}"));
    reader
        .paged(http, &ask, (MOST_MARKS, "a pull request's timeline"))
        .await
}

/// The label `label` added to and removed from pull request `number`, each
/// by the user Forgejo records, with whether its timeline was read whole.
pub(super) async fn labelled(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    number: u64,
    label: &str,
) -> Result<(Vec<Mark>, bool), Unpolled> {
    let (entries, whole) = timeline(reader, http, number).await?;
    let marks = entries
        .into_iter()
        .filter(|entry| entry.kind == LABEL)
        .filter(|entry| entry.label.as_ref().is_some_and(|held| held.name == label))
        .filter_map(|entry| {
            let into = entry.body == ADDED;
            Some(Mark {
                id: entry.id,
                into,
                by: label_actor(entry.user.map(Into::into), into)?,
                at: entry.created_at,
                named: None,
            })
        })
        .collect();
    Ok((marks, whole))
}

/// The comments on pull request `number`, with whether its timeline was
/// read whole.
pub(super) async fn commented(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    number: u64,
) -> Result<(Vec<Said>, bool), Unpolled> {
    let (entries, whole) = timeline(reader, http, number).await?;
    let said = entries
        .into_iter()
        .filter(|entry| entry.kind == COMMENT)
        .filter_map(|entry| {
            Some(Said {
                id: entry.id,
                edited: entry
                    .updated_at
                    .is_some_and(|updated| updated != entry.created_at),
                by: entry.user?.into(),
                at: entry.created_at,
                body: entry.body,
            })
        })
        .collect();
    Ok((said, whole))
}

/// The role `actor` holds on the repository; none where Forgejo knows no
/// such person.
pub(super) async fn role(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    actor: &Actor,
) -> Result<Holds, Unpolled> {
    let login = whole(&actor.login, "login")?;
    let request = reader.get(&format!("/collaborators/{login}/permission"))?;
    let read: Option<Collaborator> = reader.read(http, request, "a person's role").await?;
    Ok(read
        .and_then(|collaborator| forgejo_role(&collaborator.permission))
        .map_or(Holds::Nothing, Holds::Role))
}

/// The pushes pull request `number`'s timeline records, each forced one
/// with the head it moved from; none where the timeline was cut, since it
/// lists oldest first and a cut drops the newest.
pub(super) async fn pushes(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    number: u64,
) -> Result<Pushes, Unpolled> {
    let (entries, whole) = timeline(reader, http, number).await?;
    if !whole {
        return Ok((Vec::new(), false));
    }
    Ok((timeline_pushes(entries), true))
}

/// The pushes `entries` record.
fn timeline_pushes(entries: Vec<Entry>) -> Vec<Push> {
    entries
        .into_iter()
        .filter(|entry| entry.kind == PULL_PUSH)
        .map(|entry| {
            let pushed: Option<Pushed> = serde_json::from_str(&entry.body).ok();
            Push {
                at: entry.created_at,
                before: pushed
                    .filter(|pushed| pushed.is_force_push)
                    .and_then(|pushed| pushed.commit_ids.into_iter().next()),
            }
        })
        .collect()
}

/// The base branch `branch`: the repository's visibility, the branch's
/// head and the status checks its protection requires; Forgejo and Gitea
/// have no merge queue.
pub(super) async fn branch(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    branch: &str,
) -> Result<Branched, Unpolled> {
    let flags: Flags = reader.needed(http, reader.get("")?, "repository").await?;
    let name = whole(branch, "branch")?;
    let found: Branch = reader
        .needed(http, reader.get(&format!("/branches/{name}"))?, "branch")
        .await?;
    Ok(Branched {
        visibility: flagged(flags.private, flags.internal),
        head: found.commit.id,
        required: if found.enable_status_check {
            found.status_check_contexts
        } else {
            Vec::new()
        },
        queue: Some(false),
        queued: Queued::default(),
        whole: true,
    })
}

/// What proves `entered` green on `branched`: whether its merge base is the
/// base's head, whether Forgejo says it merges, and its head's statuses.
pub(super) async fn proved(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    entered: &Entered,
    branched: &Branched,
) -> Result<Proved, Unpolled> {
    let number = entered.number;
    let merging: Merging = reader
        .needed(
            http,
            reader.get(&format!("/pulls/{number}"))?,
            "pull request",
        )
        .await?;
    let head = whole(&entered.head, "head")?;
    let size = page(reader.forge());
    let combined: Combined = reader
        .needed(
            http,
            reader.get(&format!("/commits/{head}/status?limit={size}"))?,
            "head's statuses",
        )
        .await?;
    let whole = branched.whole && combined.statuses.len() >= combined.total_count;
    let checks: Vec<Check> = combined
        .statuses
        .into_iter()
        .map(|status| Check {
            state: if PASSING.contains(&status.status.as_str()) {
                CheckState::Passed
            } else if status.status == PENDING {
                CheckState::Pending
            } else {
                CheckState::Failed
            },
            name: status.context,
        })
        .collect();
    Ok(Proved {
        whole,
        up_to_date: merging.merge_base.as_deref() == Some(branched.head.as_str()),
        required: required(&branched.required, &checks),
        checks,
        clean: merging.mergeable,
        mergeable: merging.mergeable,
    })
}

/// The names of `checks` each of the patterns `patterns` requires, a
/// pattern no check matches kept as itself, so it is missing.
fn required(patterns: &[String], checks: &[Check]) -> Vec<String> {
    let mut names = Vec::new();
    for pattern in patterns {
        let matched: Vec<String> = Glob::new(pattern).map_or_else(
            |_| Vec::new(),
            |glob| {
                checks
                    .iter()
                    .filter(|check| glob.matches(&check.name))
                    .map(|check| check.name.clone())
                    .collect()
            },
        );
        if matched.is_empty() {
            names.push(pattern.clone());
        } else {
            names.extend(matched);
        }
    }
    names.sort();
    names.dedup();
    names
}

#[derive(Deserialize)]
struct Reviewed {
    user: Option<User>,
    state: String,
    commit_id: Option<String>,
    #[serde(default)]
    dismissed: bool,
}

/// The reviews of pull request `number`, oldest first: a dismissed review,
/// or a state the forge names that herald does not know, counts as asking
/// for changes.
pub(super) async fn reviews(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    number: u64,
) -> Result<Reviews, Unpolled> {
    let ask = |query: &str| reader.get(&format!("/pulls/{number}/reviews?{query}"));
    let (reviewed, whole): (Vec<Reviewed>, bool) = reader
        .paged(http, &ask, (MOST_MARKS, "a pull request's reviews"))
        .await?;
    let reviews = reviewed
        .into_iter()
        .filter_map(|review| {
            Some(Review {
                approves: match review.state.as_str() {
                    _ if review.dismissed => Some(false),
                    APPROVED => Some(true),
                    state if NEUTRAL.contains(&state) => None,
                    _ => Some(false),
                },
                by: review.user?.into(),
                commit: review.commit_id,
            })
        })
        .collect();
    Ok((reviews, whole))
}

#[cfg(test)]
mod tests;
