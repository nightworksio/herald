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
            queued: false,
            forge: None,
            head: self.head.map(|head| head.sha),
            unread: false,
        }
    }
}

/// The page of open pull requests `body` holds, read as `what`, parsing no
/// more than `parse` bytes, named by `named`: a repository it does not
/// know the visibility of is taken as private.
pub(super) fn page(
    what: &'static str,
    body: &[u8],
    parse: usize,
    named: Named<'_>,
) -> Result<Page, SourceError> {
    let (project, repository, known) = named;
    let listed: Vec<Listed> = herald_wire::read(what, parse, body)?;
    let seen = known.unwrap_or(Visibility::Private);
    Ok(Page {
        pulls: listed
            .into_iter()
            .map(|pull| pull.read((project, repository), seen))
            .collect(),
        next: None,
    })
}

#[cfg(test)]
mod tests;
