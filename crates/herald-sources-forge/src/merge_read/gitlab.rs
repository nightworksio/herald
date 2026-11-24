//! A merge queue's reads on GitLab.com, GitLab Self-Managed and Dedicated.

use herald_core::Permission;
use herald_core::merging::{Check, CheckState};
use herald_kinds::{Branched, Entered, Proved, Unpolled};
use herald_ports::{DynHttp, Erased};
use jiff::{SignedDuration, Timestamp};
use serde::Deserialize;

use super::{
    Actor, ForgeMergeReader, MOST_MARKS, Mark, Opened, Push, Pushes, Review, Reviews, Said,
    label_actor,
};
use crate::owner::named as visibility_named;
use crate::queue::{PIPELINE, PIPELINE_FAILED, PIPELINE_PASSED};
use crate::roles::gitlab_role;

/// The most diff versions one merge request's read takes.
const MOST_VERSIONS: usize = 100;

/// The merge states that leave the answer to the queue's own weighing:
/// GitLab still working it out, its pipeline or status checks still to
/// pass, or a rebase the queue makes.
const UNSETTLED: [&str; 8] = [
    "checking",
    "unchecked",
    "preparing",
    "approvals_syncing",
    "ci_still_running",
    "ci_must_pass",
    "status_checks_must_pass",
    "need_rebase",
];

/// The merge state of a merge request that can merge.
const MERGEABLE: &str = "mergeable";

/// GitLab's system note of an approval.
const APPROVED: &str = "approved this merge request";

/// What a merge request read on its own is called in an error.
const MERGE: &str = "the merge request";

/// What a merge request's car on the merge train is called in an error.
const CAR: &str = "the merge request's car on the merge train";

/// The state of an open merge request.
const OPENED: &str = "opened";

/// GitLab's system note of an approval taken back.
const UNAPPROVED: &str = "unapproved this merge request";

#[derive(Deserialize)]
struct User {
    id: u64,
    username: String,
}

impl From<User> for Actor {
    fn from(user: User) -> Self {
        Self {
            login: user.username,
            id: Some(user.id),
        }
    }
}

#[derive(Deserialize)]
struct Merge {
    iid: u64,
    web_url: String,
    #[serde(default)]
    draft: bool,
    sha: String,
    author: Option<User>,
    #[serde(default)]
    labels: Vec<String>,
    source_branch: Option<String>,
    #[serde(default)]
    state: String,
    target_branch: Option<String>,
    #[serde(default, rename = "merge_when_pipeline_succeeds")]
    when_pipeline_succeeds: bool,
}

impl From<Merge> for Opened {
    fn from(merge: Merge) -> Self {
        Self {
            number: merge.iid,
            url: merge.web_url,
            id: None,
            draft: merge.draft,
            head: merge.sha,
            author: merge.author.map(|user| user.username),
            labels: merge.labels,
            head_repository: None,
            head_branch: merge.source_branch,
        }
    }
}

/// A merge request's car on the merge train, of which nothing is read.
#[derive(Deserialize)]
struct Car {}

#[derive(Deserialize)]
struct Label {
    name: String,
}

#[derive(Deserialize)]
struct LabelEvent {
    #[serde(default)]
    id: Option<u64>,
    user: Option<User>,
    created_at: Timestamp,
    label: Option<Label>,
    action: String,
}

#[derive(Deserialize)]
struct Note {
    #[serde(default)]
    id: Option<u64>,
    author: Option<User>,
    #[serde(default)]
    body: String,
    created_at: Timestamp,
    updated_at: Timestamp,
    #[serde(default)]
    system: bool,
}

#[derive(Deserialize)]
struct Member {
    access_level: u64,
}

#[derive(Clone, Deserialize)]
struct Version {
    head_commit_sha: String,
    created_at: Timestamp,
}

#[derive(Deserialize)]
struct Project {
    visibility: String,
    merge_trains_enabled: Option<bool>,
    #[serde(default)]
    only_allow_merge_if_pipeline_succeeds: bool,
}

#[derive(Deserialize)]
struct Commit {
    id: String,
}

#[derive(Deserialize)]
struct Branch {
    commit: Commit,
}

#[derive(Deserialize)]
struct Pipeline {
    sha: String,
    status: String,
}

#[derive(Deserialize)]
struct Detailed {
    #[serde(rename = "detailed_merge_status")]
    status: Option<String>,
    diverged_commits_count: Option<u64>,
    head_pipeline: Option<Pipeline>,
}

/// The open merge requests into `branch`, at most [`MOST_OPENED`].
pub(super) async fn opened(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    branch: &str,
) -> Result<Vec<Opened>, Unpolled> {
    let target = herald_core::unreserved(branch);
    let ask = |query: &str| {
        reader.get(&format!(
            "/merge_requests?state=opened&target_branch={target}&{query}"
        ))
    };
    let merges: Vec<Merge> = reader.opened(http, &ask).await?;
    Ok(merges.into_iter().map(Opened::from).collect())
}

/// The merge request `number`, where it is open into `branch`.
pub(super) async fn one(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    (branch, number): (&str, u64),
) -> Result<Option<Opened>, Unpolled> {
    let merge: Option<Merge> = reader
        .read(
            http,
            reader.get(&format!("/merge_requests/{number}"))?,
            MERGE,
        )
        .await?;
    Ok(merge
        .filter(|merge| merge.state == OPENED && merge.target_branch.as_deref() == Some(branch))
        .map(Opened::from))
}

/// Whether merge request `number` is on the merge train, or set to join
/// it once its pipeline succeeds.
pub(super) async fn queued(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    number: u64,
) -> Result<bool, Unpolled> {
    let train = reader.get(&format!("/merge_trains/merge_requests/{number}"))?;
    if reader.read::<Car>(http, train, CAR).await?.is_some() {
        return Ok(true);
    }
    let merge: Merge = reader
        .needed(
            http,
            reader.get(&format!("/merge_requests/{number}"))?,
            MERGE,
        )
        .await?;
    Ok(merge.when_pipeline_succeeds)
}

/// The label `label` added to and removed from merge request `number`,
/// each by the user GitLab records, with whether its events were read
/// whole.
pub(super) async fn labelled(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    number: u64,
    label: &str,
) -> Result<(Vec<Mark>, bool), Unpolled> {
    let ask = |query: &str| {
        reader.get(&format!(
            "/merge_requests/{number}/resource_label_events?{query}"
        ))
    };
    let (events, whole): (Vec<LabelEvent>, bool) = reader
        .paged(http, &ask, (MOST_MARKS, "a merge request's label events"))
        .await?;
    let marks = events
        .into_iter()
        .filter(|event| event.label.as_ref().is_some_and(|held| held.name == label))
        .filter_map(|event| {
            let into = match event.action.as_str() {
                "add" => true,
                "remove" => false,
                _ => return None,
            };
            Some(Mark {
                id: event.id,
                by: label_actor(event.user.map(Into::into), into)?,
                at: event.created_at,
                into,
                named: None,
            })
        })
        .collect();
    Ok((marks, whole))
}

/// The comments on merge request `number`, newest first, GitLab's own
/// notes left out, with whether they were read whole.
pub(super) async fn commented(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    number: u64,
) -> Result<(Vec<Said>, bool), Unpolled> {
    let ask = |query: &str| {
        reader.get(&format!(
            "/merge_requests/{number}/notes?sort=desc&order_by=created_at&{query}"
        ))
    };
    let (notes, whole): (Vec<Note>, bool) = reader
        .paged(http, &ask, (MOST_MARKS, "a merge request's comments"))
        .await?;
    let said = notes
        .into_iter()
        .filter(|note| !note.system)
        .filter_map(|note| {
            Some(Said {
                id: note.id,
                by: note.author?.into(),
                at: note.created_at,
                body: note.body,
                edited: note.updated_at != note.created_at,
            })
        })
        .collect();
    Ok((said, whole))
}

/// The role `actor` holds on the project, directly or through a group;
/// none where they are no member, or GitLab named no id.
pub(super) async fn role(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    actor: &Actor,
) -> Result<Option<Permission>, Unpolled> {
    let Some(id) = actor.id else {
        return Ok(None);
    };
    let request = reader.get(&format!("/members/all/{id}"))?;
    let member: Option<Member> = reader.read(http, request, "a person's role").await?;
    Ok(member.and_then(|member| gitlab_role(member.access_level)))
}

/// The pushes GitLab's diff versions of merge request `number` record,
/// each with the head the one before it left.
pub(super) async fn pushes(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    number: u64,
) -> Result<Pushes, Unpolled> {
    let request = reader.get(&format!(
        "/merge_requests/{number}/versions?per_page={MOST_VERSIONS}"
    ))?;
    let versions: Vec<Version> = reader
        .needed(http, request, "merge request's diff versions")
        .await?;
    Ok(versioned(versions))
}

/// The pushes `versions` record, oldest first, and whether they are every
/// one: none where GitLab may hold more than it listed, since it does not
/// say which it leaves out.
fn versioned(mut versions: Vec<Version>) -> Pushes {
    let whole = versions.len() < MOST_VERSIONS;
    if !whole {
        return (Vec::new(), false);
    }
    versions.sort_by_key(|version| version.created_at);
    let mut before = None;
    let pushes = versions
        .into_iter()
        .map(|version| Push {
            at: version.created_at,
            before: before.replace(version.head_commit_sha),
        })
        .collect();
    (pushes, whole)
}

/// The base branch `branch`: the project's visibility, the branch's head,
/// the pipeline where the project requires one to pass, and whether merge
/// trains are on, where GitLab says.
pub(super) async fn branch(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    branch: &str,
) -> Result<Branched, Unpolled> {
    let project: Project = reader.needed(http, reader.get("")?, "project").await?;
    let name = herald_core::unreserved(branch);
    let found: Branch = reader
        .needed(
            http,
            reader.get(&format!("/repository/branches/{name}"))?,
            "branch",
        )
        .await?;
    Ok(Branched {
        visibility: visibility_named(&project.visibility),
        head: found.commit.id,
        required: if project.only_allow_merge_if_pipeline_succeeds {
            vec![PIPELINE.to_owned()]
        } else {
            Vec::new()
        },
        queue: project.merge_trains_enabled,
        whole: true,
    })
}

/// What proves `entered` green on `branched`: whether its source branch has
/// fallen behind, GitLab's merge state, and its head's pipeline.
pub(super) async fn proved(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    entered: &Entered,
    branched: &Branched,
) -> Result<Proved, Unpolled> {
    let number = entered.number;
    let detailed: Detailed = reader
        .needed(
            http,
            reader.get(&format!(
                "/merge_requests/{number}?include_diverged_commits_count=true"
            ))?,
            "merge request",
        )
        .await?;
    Ok(Proved {
        whole: branched.whole,
        up_to_date: detailed.diverged_commits_count == Some(0),
        checks: detailed
            .head_pipeline
            .filter(|pipeline| pipeline.sha == entered.head)
            .map(|pipeline| Check {
                name: PIPELINE.to_owned(),
                state: piped(&pipeline.status),
            })
            .into_iter()
            .collect(),
        required: branched.required.clone(),
        mergeable: mergeable(detailed.status.as_deref()),
    })
}

/// How a pipeline in the state `status` stands.
fn piped(status: &str) -> CheckState {
    if PIPELINE_PASSED.contains(&status) {
        CheckState::Passed
    } else if PIPELINE_FAILED.contains(&status) {
        CheckState::Failed
    } else {
        CheckState::Pending
    }
}

/// Whether GitLab's detailed merge state `state` says the merge request
/// can merge; none while it is unsettled.
fn mergeable(state: Option<&str>) -> Option<bool> {
    match state? {
        MERGEABLE => Some(true),
        state if UNSETTLED.contains(&state) => None,
        _ => Some(false),
    }
}

/// The approvals of `pull`, oldest first, each bound to the head of the
/// merge request's latest diff version where GitLab's own system note of
/// it came more than `skew` after that version, by GitLab's clock, and to
/// no head otherwise, so an approval given before the last push proves
/// nothing.
pub(super) async fn reviews(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    (pull, skew): (&Opened, SignedDuration),
) -> Result<Reviews, Unpolled> {
    let number = pull.number;
    let versions: Vec<Version> = reader
        .needed(
            http,
            reader.get(&format!(
                "/merge_requests/{number}/versions?per_page={MOST_VERSIONS}"
            ))?,
            "merge request's diff versions",
        )
        .await?;
    let latest = versions
        .iter()
        .max_by_key(|version| version.created_at)
        .filter(|_| versions.len() < MOST_VERSIONS)
        .cloned();
    let ask = |query: &str| {
        reader.get(&format!(
            "/merge_requests/{number}/notes?sort=asc&order_by=created_at&{query}"
        ))
    };
    let (notes, whole): (Vec<Note>, bool) = reader
        .paged(http, &ask, (MOST_MARKS, "a merge request's approvals"))
        .await?;
    let reviews = notes
        .into_iter()
        .filter(|note| note.system)
        .filter_map(|note| {
            let approves = match note.body.as_str() {
                APPROVED => true,
                UNAPPROVED => false,
                _ => return None,
            };
            Some(Review {
                commit: latest
                    .as_ref()
                    .filter(|version| {
                        version
                            .created_at
                            .checked_add(skew)
                            .is_ok_and(|bound| note.created_at > bound)
                    })
                    .map(|version| version.head_commit_sha.clone()),
                by: note.author?.into(),
                approves: Some(approves),
            })
        })
        .collect();
    Ok((reviews, whole))
}

#[cfg(test)]
mod tests;
