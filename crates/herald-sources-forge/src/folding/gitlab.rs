//! GitLab's deliveries of merge requests, their approvals, notes and
//! pipelines.

use herald_core::{Described, Folded, Lifecycle, Mergeable, Outcome, Verdict};
use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;

use super::{Delivery, Read, Said, keyed};
use crate::owner::levelled;
use crate::queue::{PIPELINE, forge_turn};
use crate::wire::read;

#[derive(Deserialize)]
struct Project {
    id: u64,
    path_with_namespace: String,
    web_url: String,
    visibility_level: Option<u8>,
}

#[derive(Deserialize)]
struct User {
    username: String,
}

#[derive(Deserialize)]
struct Label {
    title: String,
}

#[derive(Deserialize)]
struct Commit {
    id: String,
}

#[derive(Deserialize)]
struct Merge {
    iid: u64,
}

/// What a merge request's, a pipeline's or a note's delivery describes,
/// each part the event does not carry absent.
#[derive(Deserialize)]
struct Attributes {
    iid: Option<u64>,
    title: Option<String>,
    url: Option<String>,
    state: Option<String>,
    action: Option<String>,
    #[serde(default)]
    draft: bool,
    last_commit: Option<Commit>,
    detailed_merge_status: Option<String>,
    sha: Option<String>,
    status: Option<String>,
    noteable_type: Option<String>,
    #[serde(default, deserialize_with = "crate::gitlab::when")]
    created_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "crate::gitlab::when")]
    updated_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "crate::gitlab::when")]
    finished_at: Option<Timestamp>,
}

#[derive(Deserialize)]
struct Delivered {
    user: Option<User>,
    project: Project,
    object_attributes: Attributes,
    labels: Option<Vec<Label>>,
    reviewers: Option<Vec<User>>,
    merge_request: Option<Merge>,
}

/// What a pipeline's status says of its head.
fn piped(status: &str) -> Outcome {
    match status {
        "success" | "skipped" | "manual" => Outcome::Passed,
        "failed" | "canceled" => Outcome::Failed,
        _ => Outcome::Pending,
    }
}

/// What a merge request event's action says of a review by its actor: an
/// approval, its withdrawal, or nothing.
fn reviewed(action: &str) -> Option<Said> {
    match action {
        "approved" | "approval" => Some(Said::Gave(Verdict::Approved)),
        "unapproved" | "unapproval" => Some(Said::Withdrew),
        _ => None,
    }
}

impl Delivered {
    /// The merge request it describes, shown as `project` of the project
    /// at `path` with its visibility; none where it names no time.
    fn described(&mut self, (project, path): (&str, &str)) -> Option<Described> {
        let attributes = &mut self.object_attributes;
        let updated = attributes.updated_at.or(attributes.created_at)?;
        let opened = attributes.action.as_deref() == Some("open");
        let status = attributes.detailed_merge_status.take();
        let mergeable = match status.as_deref() {
            Some("conflict") => Some(Mergeable::Conflict),
            Some("mergeable") => Some(Mergeable::Clean),
            _ => None,
        };
        let lifecycle = match attributes.state.as_deref() {
            Some("merged") => Lifecycle::Merged,
            Some("closed") => Lifecycle::Closed,
            _ => Lifecycle::Open,
        };
        Some(Described {
            number: attributes.iid?,
            project: project.to_owned(),
            repository: path.to_owned(),
            visibility: self
                .project
                .visibility_level
                .map(|level| levelled(Some(level))),
            title: attributes.title.take().unwrap_or_default(),
            url: attributes.url.take().unwrap_or_default(),
            author: self
                .user
                .as_ref()
                .filter(|_| opened)
                .map(|user| (user.username.clone(), crate::gitlab::bot(&user.username))),
            draft: attributes.draft,
            labels: self
                .labels
                .take()
                .map(|labels| labels.into_iter().map(|label| label.title).collect()),
            reviewers: self
                .reviewers
                .take()
                .map(|reviewers| reviewers.into_iter().map(|user| user.username).collect()),
            head: attributes.last_commit.take().map(|commit| commit.id),
            mergeable,
            queue: None,
            forge: status.map(|status| (forge_turn(&status), status)),
            opened: attributes.created_at.unwrap_or(updated),
            updated,
            lifecycle,
        })
    }
}

/// What GitLab's delivery `payload` of `delivery`, read as `what`, says of
/// the merge requests of its project, shown as `project`.
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
    let mut delivered: Delivered = read(what, payload)?;
    let key = keyed(what, &delivered.project.web_url, delivered.project.id)?;
    let path = std::mem::take(&mut delivered.project.path_with_namespace);
    let mut folded = Vec::new();
    match delivery {
        Delivery::Pull => {
            let review = delivered
                .object_attributes
                .action
                .as_deref()
                .and_then(reviewed)
                .zip(delivered.user.as_ref().map(|user| user.username.clone()));
            if let Some(described) = delivered.described((project, &path)) {
                let (number, at) = (described.number, described.updated);
                folded.push(Folded::Pull(Box::new(described)));
                folded.extend(review.map(|(said, reviewer)| Folded::Review {
                    number,
                    reviewer,
                    verdict: said.verdict(),
                    decides: Some(true),
                    at,
                }));
            }
        }
        Delivery::Check => {
            let attributes = delivered.object_attributes;
            let at = attributes.finished_at.or(attributes.created_at);
            if let (Some(head), Some(status), Some(at)) = (attributes.sha, attributes.status, at) {
                folded.push(Folded::Check {
                    head,
                    name: PIPELINE.to_owned(),
                    outcome: piped(&status),
                    at,
                });
            }
        }
        Delivery::Remark
        | Delivery::Review(_)
        | Delivery::Comment
        | Delivery::Status
        | Delivery::Repository
        | Delivery::MergeGroup => {
            let attributes = delivered.object_attributes;
            let noted = attributes.noteable_type.as_deref() == Some("MergeRequest");
            let at = attributes.updated_at.or(attributes.created_at);
            if let (true, Some(merge), Some(at)) = (noted, delivered.merge_request, at) {
                folded.push(Folded::Activity {
                    number: merge.iid,
                    at,
                });
            }
        }
    }
    Ok((path, key, folded))
}
