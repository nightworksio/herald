//! A merge queue's reads on GitHub, GitHub Enterprise Server and GHE.com.

use herald_core::merging::{Check, CheckState};
use herald_kinds::{Branched, Entered, Holds, Proved, Unpolled};
use herald_ports::{DynHttp, Erased};
use jiff::Timestamp;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::{
    APPROVED, Actor, ForgeMergeReader, MOST_MARKS, Mark, OPEN, Opened, PULL, Push, Pushes, Review,
    Reviews, Said, label_actor,
};
use crate::forge::{named, whole};
use crate::owner::{flagged, named as visibility_named};
use crate::roles::GithubPermissions;

/// What a pull request's place in the merge queue is called in an error.
const QUEUE_ENTRY: &str = "the pull request's place in the merge queue";

/// The query asking whether a pull request is in the merge queue.
const QUEUED: &str = "query($owner: String!, $name: String!, $number: Int!) { repository(owner: $owner, name: $name) { pullRequest(number: $number) { isInMergeQueue } } }";

/// The query listing the pull requests in a branch's merge queue.
const IN_QUEUE: &str = "query($owner: String!, $name: String!, $branch: String!) { repository(owner: $owner, name: $name) { mergeQueue(branch: $branch) { entries(first: 100) { totalCount nodes { pullRequest { number } } } } } }";

/// What the listing of a branch's merge queue is called in an error.
const QUEUE_ENTRIES: &str = "the branch's merge queue";

/// The most check runs, and statuses, one head's read takes.
const MOST_CHECKS: usize = 100;

/// The conclusions of a completed check run that pass.
const PASSING: [&str; 3] = ["success", "neutral", "skipped"];

/// The state of a commit status still running.
const PENDING: &str = "pending";

/// The state of a commit status that passed.
const SUCCESS: &str = "success";

/// The status of a check run that ended.
const COMPLETED: &str = "completed";

/// The commit GitHub names before a branch was created.
const NONE: &str = "0000000000000000000000000000000000000000";

/// The rule of a ruleset that sends a branch's merges through its queue.
const MERGE_QUEUE: &str = "merge_queue";

/// The rule of a ruleset that requires checks.
const REQUIRED_CHECKS: &str = "required_status_checks";

/// The activity of a branch deleted, which moves no head.
const DELETION: &str = "branch_deletion";

/// The review states that neither approve nor ask for changes.
const NEUTRAL: [&str; 2] = ["COMMENTED", "PENDING"];

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
struct HeadRepository {
    full_name: String,
}

#[derive(Deserialize)]
struct Head {
    sha: String,
    #[serde(rename = "ref")]
    branch: Option<String>,
    repo: Option<HeadRepository>,
}

#[derive(Deserialize)]
struct Pull {
    number: u64,
    html_url: String,
    node_id: Option<String>,
    #[serde(default)]
    state: String,
    base: Option<PullBase>,
    #[serde(default)]
    draft: bool,
    head: Head,
    user: Option<User>,
    #[serde(default)]
    labels: Vec<Label>,
}

#[derive(Deserialize)]
struct PullBase {
    #[serde(rename = "ref")]
    branch: String,
}

impl From<Pull> for Opened {
    fn from(pull: Pull) -> Self {
        Self {
            number: pull.number,
            url: pull.html_url,
            id: pull.node_id,
            draft: pull.draft,
            head: pull.head.sha,
            author: pull.user.map(|user| user.login),
            labels: pull.labels.into_iter().map(|label| label.name).collect(),
            head_repository: pull.head.repo.map(|repo| repo.full_name),
            head_branch: pull.head.branch,
        }
    }
}

/// What GitHub's GraphQL API answers about a pull request in its merge
/// queue.
#[derive(Deserialize)]
struct Answered<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
struct QueuedData {
    repository: Option<QueuedRepository>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueuedRepository {
    pull_request: Option<QueuedPull>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueuedPull {
    is_in_merge_queue: bool,
}

#[derive(Deserialize)]
struct InQueueData {
    repository: Option<InQueueRepository>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InQueueRepository {
    merge_queue: Option<MergeQueue>,
}

#[derive(Deserialize)]
struct MergeQueue {
    entries: QueueEntries,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueEntries {
    total_count: usize,
    #[serde(default)]
    nodes: Vec<Option<QueueEntry>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueEntry {
    pull_request: Option<Numbered>,
}

#[derive(Deserialize)]
struct Numbered {
    number: u64,
}

#[derive(Deserialize)]
struct Event {
    #[serde(default)]
    id: Option<u64>,
    #[serde(rename = "event")]
    kind: String,
    actor: Option<User>,
    created_at: Timestamp,
    label: Option<Label>,
}

#[derive(Deserialize)]
struct Comment {
    #[serde(default)]
    id: Option<u64>,
    user: Option<User>,
    #[serde(default)]
    body: String,
    created_at: Timestamp,
    updated_at: Timestamp,
}

#[derive(Deserialize)]
struct Collaborator {
    user: Option<Permitted>,
}

#[derive(Deserialize)]
struct Permitted {
    permissions: Option<GithubPermissions>,
}

#[derive(Deserialize)]
struct Activity {
    before: Option<String>,
    timestamp: Timestamp,
    #[serde(rename = "activity_type")]
    kind: String,
}

#[derive(Deserialize)]
struct Repository {
    visibility: Option<String>,
    private: Option<bool>,
}

#[derive(Deserialize)]
struct Commit {
    sha: String,
}

#[derive(Deserialize, Default)]
struct Context {
    context: String,
}

#[derive(Deserialize, Default)]
struct RequiredChecks {
    #[serde(default)]
    contexts: Vec<String>,
    #[serde(default)]
    checks: Vec<Context>,
}

#[derive(Deserialize, Default)]
struct Protection {
    #[serde(default)]
    required_status_checks: Option<RequiredChecks>,
}

#[derive(Deserialize)]
struct Branch {
    commit: Commit,
    #[serde(default)]
    protected: bool,
    #[serde(default)]
    protection: Option<Protection>,
}

#[derive(Deserialize, Default)]
struct Parameters {
    #[serde(default)]
    required_status_checks: Vec<Context>,
}

#[derive(Deserialize)]
struct Rule {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    parameters: Option<Parameters>,
}

#[derive(Deserialize)]
struct Compared {
    behind_by: u64,
}

#[derive(Deserialize)]
struct Merged {
    mergeable_state: Option<String>,
}

#[derive(Deserialize)]
struct Run {
    name: String,
    status: String,
    conclusion: Option<String>,
}

#[derive(Deserialize)]
struct Runs {
    total_count: usize,
    #[serde(default)]
    check_runs: Vec<Run>,
}

#[derive(Deserialize)]
struct Status {
    context: String,
    state: String,
}

#[derive(Deserialize)]
struct Combined {
    total_count: usize,
    #[serde(default)]
    statuses: Vec<Status>,
}

/// The open pull requests into `branch`, at most [`MOST_OPENED`].
pub(super) async fn opened(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    branch: &str,
) -> Result<Vec<Opened>, Unpolled> {
    let base = herald_core::unreserved(branch);
    let ask = |query: &str| reader.get(&format!("/pulls?state=open&base={base}&{query}"));
    let pulls: Vec<Pull> = reader.opened(http, &ask).await?;
    Ok(pulls.into_iter().map(Opened::from).collect())
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
        .filter(|pull| {
            pull.state == OPEN && pull.base.as_ref().is_some_and(|base| base.branch == branch)
        })
        .map(Opened::from))
}

/// The data of GitHub's GraphQL answer to the query `query` with the
/// variable `key` set to `value` on the repository, read as `what`, where
/// it answered with no error.
async fn answered<T: DeserializeOwned>(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    (query, (key, value)): (&str, (&str, serde_json::Value)),
    what: &'static str,
) -> Result<T, Unpolled> {
    let (owner, name) =
        reader.origin.path().split_once('/').ok_or_else(|| {
            Unpolled::Unreadable(format!("{} names no owner", reader.origin.path()))
        })?;
    let mut variables = serde_json::Map::new();
    variables.insert("owner".to_owned(), owner.into());
    variables.insert("name".to_owned(), name.into());
    variables.insert(key.to_owned(), value);
    let body = serde_json::json!({ "query": query, "variables": variables });
    let request = crate::forge::authorized(
        crate::Forge::GitHub,
        crate::graphql::posted(&reader.graphql, &body, reader.origin.reading().parse),
        reader.token.as_ref(),
    );
    let answered: Answered<T> = reader.needed(http, request, what).await?;
    match answered.data {
        Some(data) if answered.errors.is_empty() => Ok(data),
        _ => Err(Unpolled::Unreadable(format!("GitHub did not say {what}"))),
    }
}

/// Whether pull request `number` is in the merge queue, by GitHub's
/// GraphQL API.
pub(super) async fn queued(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    number: u64,
) -> Result<bool, Unpolled> {
    let asked = (QUEUED, ("number", number.into()));
    let data: QueuedData = answered(reader, http, asked, QUEUE_ENTRY).await?;
    data.repository
        .and_then(|repository| repository.pull_request)
        .map(|pull| pull.is_in_merge_queue)
        .ok_or_else(|| {
            Unpolled::Unreadable(format!(
                "GitHub did not say whether pull request {number} is in the merge queue"
            ))
        })
}

/// The pull requests in `branch`'s merge queue, by GitHub's GraphQL API;
/// none where the branch has no merge queue.
pub(super) async fn in_queue(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    branch: &str,
) -> Result<Vec<u64>, Unpolled> {
    let asked = (IN_QUEUE, ("branch", branch.into()));
    let data: InQueueData = answered(reader, http, asked, QUEUE_ENTRIES).await?;
    let Some(queue) = data
        .repository
        .and_then(|repository| repository.merge_queue)
    else {
        return Ok(Vec::new());
    };
    let numbers: Vec<u64> = queue
        .entries
        .nodes
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.pull_request.map(|pull| pull.number))
        .collect();
    if numbers.len() < queue.entries.total_count {
        return Err(Unpolled::Unreadable(format!(
            "GitHub listed {} of the {} pull requests in the branch's merge queue",
            numbers.len(),
            queue.entries.total_count
        )));
    }
    Ok(numbers)
}

/// The label `label` added to and removed from pull request `number`, each
/// by the actor GitHub records, with whether its events were read whole.
pub(super) async fn labelled(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    number: u64,
    label: &str,
) -> Result<(Vec<Mark>, bool), Unpolled> {
    let ask = |query: &str| reader.get(&format!("/issues/{number}/events?{query}"));
    let (events, whole): (Vec<Event>, bool) = reader
        .paged(http, &ask, (MOST_MARKS, "a pull request's events"))
        .await?;
    Ok((label_marks(events, label), whole))
}

/// The marks of the label `label` that `events` hold, each with its actor.
fn label_marks(events: Vec<Event>, label: &str) -> Vec<Mark> {
    events
        .into_iter()
        .filter(|event| event.label.as_ref().is_some_and(|held| held.name == label))
        .filter_map(|event| {
            let into = match event.kind.as_str() {
                "labeled" => true,
                "unlabeled" => false,
                _ => return None,
            };
            Some(Mark {
                id: event.id,
                by: label_actor(event.actor.map(Into::into), into)?,
                at: event.created_at,
                into,
                named: None,
            })
        })
        .collect()
}

/// The comments on pull request `number`, with whether they were read
/// whole.
pub(super) async fn commented(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    number: u64,
) -> Result<(Vec<Said>, bool), Unpolled> {
    let ask = |query: &str| reader.get(&format!("/issues/{number}/comments?{query}"));
    let (comments, whole): (Vec<Comment>, bool) = reader
        .paged(http, &ask, (MOST_MARKS, "a pull request's comments"))
        .await?;
    let said = comments
        .into_iter()
        .filter_map(|comment| {
            Some(Said {
                id: comment.id,
                by: comment.user?.into(),
                at: comment.created_at,
                body: comment.body,
                edited: comment.updated_at != comment.created_at,
            })
        })
        .collect();
    Ok((said, whole))
}

/// The role `actor` holds on the repository; none where GitHub names none
/// or knows no such person.
pub(super) async fn role(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    actor: &Actor,
) -> Result<Holds, Unpolled> {
    let login = whole(&actor.login, "login")?;
    let request = reader.get(&format!("/collaborators/{login}/permission"))?;
    let read: Option<Collaborator> = reader.read(http, request, "a person's role").await?;
    Ok(match read {
        None => Holds::Nothing,
        Some(Collaborator {
            user: Some(Permitted {
                permissions: Some(permissions),
            }),
        }) => permissions.role().map_or(Holds::Nothing, Holds::Role),
        Some(_) => Holds::Unsaid,
    })
}

/// The pushes GitHub's activity of the repository holding `pull`'s head
/// records to its branch: none listed where the head's repository is gone
/// or GitHub would not say.
pub(super) async fn pushes(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    pull: &Opened,
) -> Result<Pushes, Unpolled> {
    let (Some(repository), Some(branch)) = (&pull.head_repository, &pull.head_branch) else {
        return Ok((Vec::new(), false));
    };
    let path = format!(
        "/repos/{}/activity?ref={}&direction=desc&per_page={MOST_CHECKS}",
        named(repository, "repository")?,
        herald_core::unreserved(&format!("refs/heads/{branch}"))
    );
    let read: Option<Vec<Activity>> = reader
        .read(http, reader.rooted(&path), "a branch's pushes")
        .await?;
    let Some(activities) = read else {
        return Ok((Vec::new(), false));
    };
    Ok(activity_pushes(activities))
}

/// The pushes `activities` record, never taken as every one: GitHub's
/// activity may be cut by age, so a gesture is placed only after a push
/// it lists.
fn activity_pushes(activities: Vec<Activity>) -> Pushes {
    let pushes = activities
        .into_iter()
        .filter(|activity| activity.kind != DELETION)
        .map(|activity| Push {
            at: activity.timestamp,
            before: activity.before.filter(|before| before != NONE),
        })
        .collect();
    (pushes, false)
}

/// The base branch `branch`: the repository's visibility, the branch's
/// head, the checks its protection and its rulesets require, and whether a
/// ruleset sends its merges through GitHub's merge queue; none said of the
/// queue where classic branch protection, which may hold one GitHub's REST
/// API does not show, protects it.
pub(super) async fn branch(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    branch: &str,
) -> Result<Branched, Unpolled> {
    let repository: Repository = reader.needed(http, reader.get("")?, "repository").await?;
    let name = whole(branch, "branch")?;
    let found: Branch = reader
        .needed(http, reader.get(&format!("/branches/{name}"))?, "branch")
        .await?;
    let ask = |query: &str| reader.get(&format!("/rules/branches/{name}?{query}"));
    let (rules, whole): (Vec<Rule>, bool) = reader
        .paged(http, &ask, (MOST_CHECKS, "a branch's rules"))
        .await?;
    Ok(branched(repository, found, (rules, whole)))
}

/// What `repository`, its branch `found` and the `rules` on it say, `whole`
/// where the rules were read whole.
fn branched(repository: Repository, found: Branch, (rules, whole): (Vec<Rule>, bool)) -> Branched {
    let visibility = repository.visibility.map_or_else(
        || flagged(repository.private, None),
        |name| visibility_named(&name),
    );
    let queued = rules.iter().any(|rule| rule.kind == MERGE_QUEUE);
    let protected = found
        .protection
        .unwrap_or_default()
        .required_status_checks
        .unwrap_or_default();
    let mut required: Vec<String> = protected
        .contexts
        .into_iter()
        .chain(protected.checks.into_iter().map(|check| check.context))
        .chain(
            rules
                .into_iter()
                .filter(|rule| rule.kind == REQUIRED_CHECKS)
                .flat_map(|rule| rule.parameters.unwrap_or_default().required_status_checks)
                .map(|check| check.context),
        )
        .collect();
    required.sort();
    required.dedup();
    Branched {
        visibility,
        head: found.commit.sha,
        required,
        queue: if queued {
            Some(true)
        } else {
            (!found.protected).then_some(false)
        },
        whole,
    }
}

/// What proves `entered` green on `branched`: whether its head holds the
/// base's, what GitHub's merge state says, and the newest check runs and
/// statuses on its head, never whole where GitHub holds more of them than
/// one page lists.
pub(super) async fn proved(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    entered: &Entered,
    branched: &Branched,
) -> Result<Proved, Unpolled> {
    let head = whole(&entered.head, "head")?;
    let base = whole(&branched.head, "head")?;
    let number = entered.number;
    let compared: Compared = reader
        .needed(
            http,
            reader.get(&format!("/compare/{base}...{head}?per_page=1"))?,
            "comparison of a head with its base",
        )
        .await?;
    let merged: Merged = reader
        .needed(
            http,
            reader.get(&format!("/pulls/{number}"))?,
            "pull request",
        )
        .await?;
    let runs: Runs = reader
        .needed(
            http,
            reader.get(&format!(
                "/commits/{head}/check-runs?per_page={MOST_CHECKS}"
            ))?,
            "head's check runs",
        )
        .await?;
    let combined: Combined = reader
        .needed(
            http,
            reader.get(&format!("/commits/{head}/status?per_page={MOST_CHECKS}"))?,
            "head's statuses",
        )
        .await?;
    let whole = branched.whole
        && runs.check_runs.len() >= runs.total_count
        && combined.statuses.len() >= combined.total_count;
    Ok(Proved {
        whole,
        up_to_date: compared.behind_by == 0,
        checks: checks(runs, combined),
        required: branched.required.clone(),
        mergeable: mergeable(merged.mergeable_state.as_deref()),
    })
}

/// The checks `runs` and `combined` report.
fn checks(runs: Runs, combined: Combined) -> Vec<Check> {
    let ran = runs.check_runs.into_iter().map(|run| Check {
        state: if run.status != COMPLETED {
            CheckState::Pending
        } else if run
            .conclusion
            .as_deref()
            .is_some_and(|conclusion| PASSING.contains(&conclusion))
        {
            CheckState::Passed
        } else {
            CheckState::Failed
        },
        name: run.name,
    });
    let stated = combined.statuses.into_iter().map(|status| Check {
        state: match status.state.as_str() {
            PENDING => CheckState::Pending,
            SUCCESS => CheckState::Passed,
            _ => CheckState::Failed,
        },
        name: status.context,
    });
    ran.chain(stated).collect()
}

/// Whether GitHub's merge state `state` says the pull request can merge:
/// clean, or held only by checks the queue weighs itself; none while
/// GitHub works it out, or where it is behind, which the queue mends.
fn mergeable(state: Option<&str>) -> Option<bool> {
    match state? {
        "clean" | "has_hooks" | "unstable" => Some(true),
        "dirty" | "blocked" | "draft" => Some(false),
        _ => None,
    }
}

#[derive(Deserialize)]
struct Reviewed {
    user: Option<User>,
    state: String,
    commit_id: Option<String>,
}

/// The reviews of pull request `number`, oldest first: a state GitHub
/// names that herald does not know counts as asking for changes.
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
