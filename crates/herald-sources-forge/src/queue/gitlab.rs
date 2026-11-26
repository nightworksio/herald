use super::Titled;
use herald_core::{Checks, Mergeable, Pull, Review, Turn, Visibility};
use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;

use super::{Named, PAGE, Page};
use crate::gitlab::bot;

/// What GitLab's open merge requests are read as.
pub(super) const WHAT: &str = "GitLab's open merge requests";

/// The one check GitLab's merge requests are read with: the head's
/// pipeline.
pub(crate) const PIPELINE: &str = "pipeline";

/// The path, under a project, of the `number`th page of its open merge
/// requests, the most recently updated first.
pub(super) fn path(number: usize) -> String {
    format!(
        "/merge_requests?state=opened&order_by=updated_at&sort=desc&per_page={PAGE}&page={number}"
    )
}

/// The path, under the API's base URL, of the `number`th page of the open
/// merge requests of every project of the group `group` and its
/// subgroups, the most recently updated first (ADR-0062).
pub(super) fn group_path(group: &str, number: usize) -> String {
    format!(
        "/groups/{}/merge_requests?state=opened&scope=all&order_by=updated_at&sort=desc&per_page={PAGE}&page={number}",
        herald_core::unreserved(group)
    )
}

#[derive(Deserialize)]
struct User {
    username: String,
}

#[derive(Deserialize)]
struct Request {
    iid: u64,
    title: String,
    web_url: String,
    #[serde(default)]
    draft: bool,
    created_at: Timestamp,
    updated_at: Timestamp,
    author: Option<User>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    reviewers: Vec<User>,
    #[serde(default)]
    has_conflicts: bool,
    detailed_merge_status: Option<String>,
    sha: Option<String>,
    project_id: Option<u64>,
    target_branch: Option<String>,
    source_branch: Option<String>,
    milestone: Option<Titled>,
}

/// What GitLab's detailed merge status says of the reviews and checks.
fn read_status(status: &str, reviewers: bool) -> (Review, Checks) {
    let unreviewed = if reviewers {
        Review::Requested
    } else {
        Review::Unasked
    };
    match status {
        "mergeable" => (Review::Approved, Checks::Passed),
        "requested_changes" => (Review::ChangesRequested, Checks::Absent),
        "ci_still_running" => (unreviewed, Checks::Pending),
        "ci_must_pass" => (unreviewed, Checks::Failed(vec![PIPELINE.to_owned()])),
        _ => (unreviewed, Checks::Absent),
    }
}

/// The turn GitLab's detailed merge status maps to.
pub(crate) fn forge_turn(status: &str) -> Turn {
    match status {
        "mergeable" => Turn::WaitingOnMerge,
        "draft_status" => Turn::Draft,
        "ci_still_running" | "checking" | "unchecked" | "preparing" | "approvals_syncing" => {
            Turn::WaitingOnChecks
        }
        "ci_must_pass"
        | "conflict"
        | "need_rebase"
        | "requested_changes"
        | "discussions_not_resolved" => Turn::WaitingOnAuthor,
        "not_approved" => Turn::WaitingOnReview,
        _ => Turn::Unknown,
    }
}

impl Request {
    /// The merge request, of `project` at `repository`, seen by `seen`.
    fn read(self, (project, repository): (&str, &str), seen: Visibility) -> Pull {
        let status = self.detailed_merge_status.unwrap_or_default();
        let (review, checks) = read_status(&status, !self.reviewers.is_empty());
        let mergeable = if self.has_conflicts || status == "conflict" {
            Mergeable::Conflict
        } else if status == "mergeable" {
            Mergeable::Clean
        } else {
            Mergeable::Unknown
        };
        Pull {
            project: project.to_owned(),
            repository: repository.to_owned(),
            visibility: seen,
            number: self.iid,
            title: self.title,
            url: self.web_url,
            bot: self
                .author
                .as_ref()
                .is_some_and(|author| bot(&author.username)),
            author: self.author.map(|author| author.username),
            draft: self.draft,
            labels: self.labels,
            opened: self.created_at,
            updated: self.updated_at,
            review,
            reviewers: self
                .reviewers
                .into_iter()
                .map(|reviewer| reviewer.username)
                .collect(),
            checks,
            mergeable,
            queue: None,
            forge: (!status.is_empty()).then(|| (forge_turn(&status), status)),
            head: self.sha,
            unread: false,
            base: self.target_branch,
            files: None,
            areas: Vec::new(),
            head_branch: self.source_branch,
            milestone: self.milestone.map(|milestone| milestone.title),
            flips: Vec::new(),
        }
    }
}

/// The page of open merge requests `body` holds, parsing no more than
/// `parse` bytes, named by `named`: a project it does not know the
/// visibility of is taken as private.
pub(super) fn page(body: &[u8], parse: usize, named: Named<'_>) -> Result<Page, SourceError> {
    let (project, repository, known) = named;
    let requests: Vec<Request> = herald_wire::read(WHAT, parse, body)?;
    let seen = known.unwrap_or(Visibility::Private);
    let id = requests
        .iter()
        .find_map(|request| request.project_id)
        .map(|id| id.to_string());
    Ok(Page {
        pulls: requests
            .into_iter()
            .map(|request| request.read((project, repository), seen))
            .collect(),
        next: None,
        repository: id,
        visibility: None,
        renamed: super::Renamed::new(),
    })
}

/// The open merge requests of a group's projects `body` holds, parsing no
/// more than `parse` bytes, each with the id of its project, and how many
/// it held in all; one naming no project is left out (ADR-0062).
pub(super) fn grouped(
    body: &[u8],
    parse: usize,
) -> Result<(Vec<(String, Pull)>, usize), SourceError> {
    let requests: Vec<Request> = herald_wire::read(WHAT, parse, body)?;
    let items = requests.len();
    let pulls = requests
        .into_iter()
        .filter_map(|request| {
            let id = request.project_id?.to_string();
            Some((id, request.read(("", ""), Visibility::Private)))
        })
        .collect();
    Ok((pulls, items))
}

#[cfg(test)]
mod tests;
