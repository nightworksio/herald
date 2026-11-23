//! GitHub's and Forgejo's milestones, which both describe alike but for
//! GitHub's number and page.

use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;

use super::{ForgeMilestone, Milestone, MilestoneAction};
use crate::Forge;
use crate::forge::Repository;
use crate::wire::read;

/// The name GitHub gives its milestone webhook's deliveries.
pub(super) const HOOK: &str = "milestone";

/// A milestone, as GitHub and Forgejo describe it.
#[derive(Deserialize)]
struct Described {
    id: u64,
    number: Option<u64>,
    title: String,
    description: Option<String>,
    state: String,
    open_issues: Option<u32>,
    closed_issues: Option<u32>,
    created_at: Timestamp,
    updated_at: Option<Timestamp>,
    closed_at: Option<Timestamp>,
    due_on: Option<Timestamp>,
    html_url: Option<String>,
}

impl Described {
    /// The milestone on `forge`, read as `what`: GitHub reads it by its
    /// number, Forgejo by its id.
    fn read(self, forge: Forge, what: &'static str) -> Result<Milestone, SourceError> {
        let key = match forge {
            Forge::GitHub => self.number.ok_or(SourceError::Unreadable {
                what,
                reason: "the milestone names no number".to_owned(),
            })?,
            Forge::GitLab | Forge::Forgejo => self.id,
        };
        Ok(Milestone {
            key,
            number: key,
            group: None,
            title: self.title,
            description: self.description,
            open: self.state == "open",
            due: self
                .due_on
                .map(|due| due.to_zoned(jiff::tz::TimeZone::UTC).date().to_string()),
            counts: self.open_issues.zip(self.closed_issues),
            created: self.created_at,
            updated: self.updated_at,
            closed: self.closed_at,
            url: self.html_url,
            path: format!("/milestone/{key}"),
        })
    }
}

/// What a milestone event's action says of it: GitHub's `opened` is a
/// closed milestone opened again, and an edit is not announced.
fn hooked_action(action: &str) -> Option<MilestoneAction> {
    match action {
        "created" => Some(MilestoneAction::Opened),
        "closed" => Some(MilestoneAction::Closed),
        "opened" => Some(MilestoneAction::Reopened),
        "deleted" => Some(MilestoneAction::Deleted),
        _ => None,
    }
}

/// A repository, as GitHub's events name it.
#[derive(Deserialize)]
struct Named {
    id: u64,
    name: String,
    full_name: String,
    html_url: Option<String>,
}

/// GitHub's milestone event.
#[derive(Deserialize)]
struct Hooked {
    action: String,
    milestone: Described,
    repository: Named,
}

/// What the milestone event `payload`, read as `what`, names: the
/// repository's full path, the repository, and the milestone where its
/// action is one herald announces.
pub(super) fn delivered(
    what: &'static str,
    payload: &[u8],
) -> Result<(String, Repository, Option<ForgeMilestone>), SourceError> {
    let hooked: Hooked = read(what, payload)?;
    let repository = hooked.repository;
    let milestone = match hooked_action(&hooked.action) {
        Some(action) => Some(ForgeMilestone {
            milestone: hooked.milestone.read(Forge::GitHub, what)?,
            action,
        }),
        None => None,
    };
    Ok((
        repository.full_name,
        Repository {
            id: repository.id,
            name: repository.name,
            page: repository.html_url,
        },
        milestone,
    ))
}

/// The milestones in `forge`'s listing `body`, read as `what`, parsing no
/// more than `limit` bytes.
pub(super) fn listed(
    forge: Forge,
    what: &'static str,
    limit: usize,
    body: &[u8],
) -> Result<Vec<Milestone>, SourceError> {
    let described: Vec<Described> = herald_wire::read(what, limit, body)?;
    described
        .into_iter()
        .map(|milestone| milestone.read(forge, what))
        .collect()
}

/// The one milestone `forge`'s `body` describes, read as `what`, parsing no
/// more than `limit` bytes.
pub(super) fn one(
    forge: Forge,
    what: &'static str,
    limit: usize,
    body: &[u8],
) -> Result<Milestone, SourceError> {
    herald_wire::read::<Described>(what, limit, body)?.read(forge, what)
}
