//! GitLab's milestones: a project's own, and those of the group holding it.

use herald_core::unreserved;
use herald_kinds::Unpolled;
use herald_ports::Request;
use herald_wire::SourceError;
use jiff::Timestamp;
use jiff::civil::Date;
use serde::Deserialize;

use super::{ForgeMilestone, Milestone, MilestoneAction};
use crate::forge::Repository;
use crate::wire::read;
use crate::{Origin, Token};

/// The name GitLab gives its milestone webhook's deliveries.
pub(super) const HOOK: &str = "Milestone Hook";
/// What GitLab's counts of a milestone's issues are read as.
pub(super) const STATISTICS: &str = "GitLab's issue statistics";
/// What the group holding a project is named as where its path is not one.
const GROUP: &str = "the group holding the project";

/// A milestone, as GitLab's API and webhook describe it.
#[derive(Deserialize)]
struct Described {
    id: u64,
    iid: u64,
    title: String,
    description: Option<String>,
    state: String,
    project_id: Option<u64>,
    group_id: Option<u64>,
    due_date: Option<Date>,
    #[serde(default, deserialize_with = "crate::gitlab::when")]
    created_at: Option<Timestamp>,
    #[serde(default, deserialize_with = "crate::gitlab::when")]
    updated_at: Option<Timestamp>,
    web_url: Option<String>,
}

impl Described {
    /// The milestone, read as `what`.
    fn read(self, what: &'static str) -> Result<Milestone, SourceError> {
        let created = self
            .created_at
            .or(self.updated_at)
            .ok_or(SourceError::Unreadable {
                what,
                reason: "the milestone names no time".to_owned(),
            })?;
        Ok(Milestone {
            key: self.id,
            number: self.iid,
            group: self.group_id.filter(|_| self.project_id.is_none()),
            title: self.title,
            description: self.description,
            open: self.state == "active",
            due: self.due_date.map(|due| due.to_string()),
            counts: None,
            created,
            updated: self.updated_at,
            closed: None,
            url: self.web_url,
            path: format!("/-/milestones/{}", self.iid),
        })
    }
}

/// What a milestone event's action says of it.
fn hooked_action(action: &str) -> Option<MilestoneAction> {
    match action {
        "create" => Some(MilestoneAction::Opened),
        "close" => Some(MilestoneAction::Closed),
        "reopen" => Some(MilestoneAction::Reopened),
        "delete" => Some(MilestoneAction::Deleted),
        _ => None,
    }
}

/// A project, as GitLab's webhook names it.
#[derive(Deserialize)]
struct Project {
    id: u64,
    path_with_namespace: String,
    web_url: Option<String>,
}

/// GitLab's milestone event.
#[derive(Deserialize)]
struct Hooked {
    action: Option<String>,
    project: Option<Project>,
    object_attributes: Described,
}

/// What GitLab's milestone event `payload` names: the project's full path,
/// the project, and the milestone where its action is one herald
/// announces; `None` where it names no project, as a group's own webhook
/// does.
pub(super) fn delivered(
    payload: &[u8],
) -> Result<Option<(String, Repository, Option<ForgeMilestone>)>, SourceError> {
    let what = super::HOOKED.gitlab;
    let hooked: Hooked = read(what, payload)?;
    let Some(project) = hooked.project else {
        return Ok(None);
    };
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
    let milestone = match hooked_action(hooked.action.as_deref().unwrap_or_default()) {
        Some(action) => Some(ForgeMilestone {
            milestone: hooked.object_attributes.read(what)?,
            action,
        }),
        None => None,
    };
    Ok(Some((project.path_with_namespace, repository, milestone)))
}

/// The milestones in GitLab's listing `body`, read as `what`, parsing no
/// more than `limit` bytes.
pub(super) fn listed(
    what: &'static str,
    limit: usize,
    body: &[u8],
) -> Result<Vec<Milestone>, SourceError> {
    let described: Vec<Described> = herald_wire::read(what, limit, body)?;
    described
        .into_iter()
        .map(|milestone| milestone.read(what))
        .collect()
}

/// The one milestone GitLab's `body` describes, read as `what`, parsing no
/// more than `limit` bytes.
pub(super) fn one(what: &'static str, limit: usize, body: &[u8]) -> Result<Milestone, SourceError> {
    herald_wire::read::<Described>(what, limit, body)?.read(what)
}

/// The request for `path` under the group holding the project `origin`
/// names, carrying `token`, the group's path through the one segment
/// encoder.
///
/// # Errors
///
/// Where the project is held by no group, or a part of the group's path is
/// empty, `.` or `..`.
pub(super) fn group_request(
    origin: &Origin,
    token: Option<&Token>,
    path: &str,
) -> Result<Request, Unpolled> {
    let group = origin
        .path()
        .rsplit_once('/')
        .map(|(group, _)| group)
        .unwrap_or_default();
    let group = crate::forge::named(group, GROUP)?;
    Ok(origin.rooted(&format!("/groups/{}{path}", unreserved(&group)), token))
}

/// The path, under a project or group, of the counts of the issues of its
/// milestone titled `title`.
pub(super) fn statistics_path(title: &str) -> String {
    format!("/issues_statistics?milestone={}", unreserved(title))
}

/// GitLab's counts of a milestone's issues.
#[derive(Deserialize)]
struct Statistics {
    statistics: Counted,
}

/// The counts in GitLab's issue statistics.
#[derive(Deserialize)]
struct Counted {
    counts: Counts,
}

/// How many issues are open and closed.
#[derive(Deserialize)]
struct Counts {
    opened: u32,
    closed: u32,
}

/// How many of a milestone's issues are open and closed, as GitLab's
/// statistics `body` count them, parsing no more than `limit` bytes.
pub(super) fn counted(limit: usize, body: &[u8]) -> Result<(u32, u32), SourceError> {
    let counts = herald_wire::read::<Statistics>(STATISTICS, limit, body)?
        .statistics
        .counts;
    Ok((counts.opened, counts.closed))
}
