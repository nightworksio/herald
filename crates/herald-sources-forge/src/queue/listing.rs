use herald_core::{Checks, Mergeable, Pull, Review, Visibility};
use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;

use crate::github::{Account, Label};

use super::{Named, PAGE, Page};
use crate::Forge;

/// What GitHub's open pull requests are read as, listed without GraphQL.
pub(super) const GITHUB: &str = "GitHub's listed open pull requests";

/// What Forgejo's open pull requests are read as.
pub(super) const FORGEJO: &str = "Forgejo's open pull requests";

/// The path, under a repository on `forge`, of the `number`th page of its
/// open pull requests, the most recently updated first.
pub(super) fn path(forge: Forge, number: usize) -> String {
    match forge {
        Forge::Forgejo => {
            format!("/pulls?state=open&sort=recentupdate&limit={PAGE}&page={number}")
        }
        Forge::GitHub | Forge::GitLab => {
            format!("/pulls?state=open&sort=updated&direction=desc&per_page={PAGE}&page={number}")
        }
    }
}

#[derive(Deserialize)]
struct Head {
    sha: String,
}

#[derive(Deserialize)]
struct Repository {
    id: u64,
    visibility: Option<String>,
    private: Option<bool>,
    internal: Option<bool>,
}

impl Repository {
    /// Who may see it, where the listing says.
    fn visibility(&self) -> Option<Visibility> {
        self.visibility
            .as_deref()
            .map(crate::owner::named)
            .or_else(|| {
                self.private
                    .map(|private| crate::owner::flagged(Some(private), self.internal))
            })
    }
}

#[derive(Deserialize)]
struct Base {
    repo: Option<Repository>,
    #[serde(rename = "ref")]
    branch: Option<String>,
}

#[derive(Deserialize)]
struct Listed {
    number: u64,
    title: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    created_at: Timestamp,
    updated_at: Timestamp,
    user: Option<Account>,
    #[serde(default)]
    labels: Vec<Label>,
    #[serde(default)]
    requested_reviewers: Vec<Account>,
    mergeable: Option<bool>,
    head: Option<Head>,
    base: Option<Base>,
}

impl Listed {
    /// The pull request, of `project` at `repository`, seen by `seen`.
    fn read(self, (project, repository): (&str, &str), seen: Visibility) -> Pull {
        let review = if self.requested_reviewers.is_empty() {
            Review::Unasked
        } else {
            Review::Requested
        };
        let mergeable = match self.mergeable {
            Some(true) => Mergeable::Clean,
            Some(false) => Mergeable::Conflict,
            None => Mergeable::Unknown,
        };
        Pull {
            project: project.to_owned(),
            repository: repository.to_owned(),
            visibility: seen,
            number: self.number,
            title: self.title,
            url: self.html_url,
            bot: self.user.as_ref().is_some_and(Account::bot),
            author: self.user.map(|user| user.login),
            draft: self.draft,
            labels: self.labels.into_iter().map(|label| label.name).collect(),
            opened: self.created_at,
            updated: self.updated_at,
            review,
            reviewers: self
                .requested_reviewers
                .into_iter()
                .map(|reviewer| reviewer.login)
                .collect(),
            checks: Checks::Absent,
            mergeable,
            queue: None,
            forge: None,
            head: self.head.map(|head| head.sha),
            unread: false,
            base: self.base.and_then(|base| base.branch),
            files: None,
            areas: Vec::new(),
            flips: Vec::new(),
        }
    }
}

/// The page of open pull requests `body` holds, read as `what`, parsing no
/// more than `parse` bytes, named by `named`: a repository its source does
/// not know the visibility of is taken as private, and what the listing
/// names of it is kept apart, to narrow kept records with alone.
pub(super) fn page(
    what: &'static str,
    body: &[u8],
    parse: usize,
    named: Named<'_>,
) -> Result<Page, SourceError> {
    let (project, repository, known) = named;
    let listed: Vec<Listed> = herald_wire::read(what, parse, body)?;
    let listed_repository = listed
        .iter()
        .find_map(|pull| pull.base.as_ref()?.repo.as_ref());
    let seen = known.unwrap_or(Visibility::Private);
    let id = listed_repository.map(|repository| repository.id.to_string());
    let named = listed_repository.and_then(Repository::visibility);
    Ok(Page {
        pulls: listed
            .into_iter()
            .map(|pull| pull.read((project, repository), seen))
            .collect(),
        next: None,
        repository: id,
        visibility: named,
    })
}

/// The path, under a repository, of pull request `number`.
pub(super) fn one_path(number: u64) -> String {
    format!("/pulls/{number}")
}

/// `pull` with what the pull request `body` holds, read as `what` and
/// parsing no more than `parse` bytes, says of its head, draft,
/// mergeability and requested reviewers, where a listing that read none
/// of them found it (ADR-0062).
pub(super) fn one(
    what: &'static str,
    body: &[u8],
    parse: usize,
    pull: &mut Pull,
) -> Result<(), SourceError> {
    let listed: Listed = herald_wire::read(what, parse, body)?;
    let read = listed.read(("", ""), Visibility::Private);
    pull.head = read.head;
    pull.draft = read.draft;
    pull.mergeable = read.mergeable;
    pull.review = read.review;
    pull.reviewers = read.reviewers;
    pull.base = read.base;
    Ok(())
}

#[cfg(test)]
mod tests;
