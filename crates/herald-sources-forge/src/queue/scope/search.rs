//! The pull requests an owner's scope query lists on GitHub and Forgejo:
//! issue searches, each item naming its repository (ADR-0062).

use herald_core::{Checks, Mergeable, Pull, Review, Visibility};
use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;

use crate::github::{Account, Label};

/// What GitHub's search of an owner's open pull requests is read as.
pub(super) const GITHUB: &str = "GitHub's search of an owner's open pull requests";

/// What Forgejo's search of an owner's open pull requests is read as.
pub(super) const FORGEJO: &str = "Forgejo's search of an owner's open pull requests";

/// What GitHub's search answers.
#[derive(Deserialize)]
struct Searched {
    total_count: usize,
    #[serde(default)]
    items: Vec<Found>,
}

/// A pull request as GitHub's search lists it.
#[derive(Deserialize)]
struct Found {
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
    repository_url: String,
}

/// A repository as Forgejo's issue search names it.
#[derive(Deserialize)]
struct Holder {
    id: u64,
}

/// What Forgejo's issue search says of an item that is a pull request.
#[derive(Deserialize)]
struct Meta {
    #[serde(default)]
    draft: bool,
}

/// A pull request as Forgejo's issue search lists it.
#[derive(Deserialize)]
struct Issue {
    number: u64,
    title: String,
    html_url: String,
    created_at: Timestamp,
    updated_at: Timestamp,
    user: Option<Account>,
    #[serde(default)]
    labels: Vec<Label>,
    repository: Option<Holder>,
    pull_request: Option<Meta>,
}

/// Where a listed pull request's repository is, as its listing names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Located {
    /// By the forge's id of it.
    Id(String),
    /// By its full path.
    Path(String),
}

/// A listed pull request, of no project and seen as private until its
/// repository is found among the owner's.
fn bare(
    (number, title, url): (u64, String, String),
    (opened, updated): (Timestamp, Timestamp),
    draft: bool,
    user: Option<Account>,
    labels: Vec<Label>,
) -> Pull {
    Pull {
        project: String::new(),
        repository: String::new(),
        visibility: Visibility::Private,
        number,
        title,
        url,
        bot: user.as_ref().is_some_and(Account::bot),
        author: user.map(|user| user.login),
        draft,
        labels: labels.into_iter().map(|label| label.name).collect(),
        opened,
        updated,
        review: Review::Unasked,
        reviewers: Vec::new(),
        checks: Checks::Absent,
        mergeable: Mergeable::Unknown,
        queued: false,
        forge: None,
        head: None,
        unread: false,
    }
}

/// The full path of the repository GitHub's API names `url`: its last two
/// segments.
fn path_of(url: &str) -> Option<String> {
    let mut segments = url.trim_end_matches('/').rsplit('/');
    let name = segments.next().filter(|name| !name.is_empty())?;
    let owner = segments.next().filter(|owner| !owner.is_empty())?;
    Some(format!("{owner}/{name}"))
}

/// What a page of a scope query lists: each pull request whose repository
/// it names, how many items it held in all, and how many the query found
/// in all where it says.
pub(super) type Listing = (Vec<(Located, Pull)>, usize, Option<usize>);

/// The pull requests a page of GitHub's search `body` lists, parsing no
/// more than `parse` bytes; an item whose repository cannot be told is
/// left out.
pub(super) fn github(body: &[u8], parse: usize) -> Result<Listing, SourceError> {
    let searched: Searched = herald_wire::read(GITHUB, parse, body)?;
    let items = searched.items.len();
    let pulls = searched
        .items
        .into_iter()
        .filter_map(|found| {
            let path = path_of(&found.repository_url)?;
            let pull = bare(
                (found.number, found.title, found.html_url),
                (found.created_at, found.updated_at),
                found.draft,
                found.user,
                found.labels,
            );
            Some((Located::Path(path), pull))
        })
        .collect();
    Ok((pulls, items, Some(searched.total_count)))
}

/// The pull requests a page of Forgejo's issue search `body` lists,
/// parsing no more than `parse` bytes; an item that is not a pull request
/// or names no repository is left out.
pub(super) fn forgejo(body: &[u8], parse: usize) -> Result<Listing, SourceError> {
    let issues: Vec<Issue> = herald_wire::read(FORGEJO, parse, body)?;
    let items = issues.len();
    let pulls = issues
        .into_iter()
        .filter_map(|issue| {
            let meta = issue.pull_request?;
            let id = issue.repository?.id.to_string();
            let pull = bare(
                (issue.number, issue.title, issue.html_url),
                (issue.created_at, issue.updated_at),
                meta.draft,
                issue.user,
                issue.labels,
            );
            Some((Located::Id(id), pull))
        })
        .collect();
    Ok((pulls, items, None))
}

#[cfg(test)]
mod tests;
