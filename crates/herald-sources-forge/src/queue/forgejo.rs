use herald_core::{Checks, Mergeable, Pull, Review, Visibility};
use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;

use super::{Named, PAGE, Page};

/// What Forgejo's open pull requests are read as.
pub(super) const WHAT: &str = "Forgejo's open pull requests";

/// What a bot's login ends with on Forgejo, which names no account kind.
const BOT_SUFFIX: &str = "[bot]";

/// The path, under a repository, of the `number`th page of its open pull
/// requests, the most recently updated first.
pub(super) fn path(number: usize) -> String {
    format!("/pulls?state=open&sort=recentupdate&limit={PAGE}&page={number}")
}

#[derive(Deserialize)]
struct User {
    login: String,
}

#[derive(Deserialize)]
struct Label {
    name: String,
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
    user: Option<User>,
    #[serde(default)]
    labels: Vec<Label>,
    #[serde(default)]
    requested_reviewers: Vec<User>,
    mergeable: Option<bool>,
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
            bot: self
                .user
                .as_ref()
                .is_some_and(|user| user.login.ends_with(BOT_SUFFIX)),
            author: self.user.map(|user| user.login),
            draft: self.draft,
            labels: self.labels.into_iter().map(|label| label.name).collect(),
            opened: self.created_at,
            updated: self.updated_at,
            review,
            checks: Checks::Absent,
            mergeable,
            queued: false,
            forge: None,
        }
    }
}

/// The page of open pull requests `body` holds, parsing no more than
/// `parse` bytes, named by `named`: a repository it does not know the
/// visibility of is taken as private.
pub(super) fn page(body: &[u8], parse: usize, named: Named<'_>) -> Result<Page, SourceError> {
    let (project, repository, known) = named;
    let listed: Vec<Listed> = herald_wire::read(WHAT, parse, body)?;
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
