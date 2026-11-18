use herald_core::{Checks, Mergeable, Pull, Review, Turn, Visibility};
use herald_ports::Request;
use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;
use serde_json::json;

use super::{FAILED, Named, PAGE, Page};
use crate::graphql::posted;

/// What GitHub's open pull requests are read as.
pub(super) const WHAT: &str = "GitHub's open pull requests";

/// The most labels and check contexts a pull request is read with.
const MOST_NAMED: usize = 50;

/// The query for one page of a repository's open pull requests, the most
/// recently updated first.
fn query() -> String {
    format!(
        "query($owner: String!, $name: String!, $after: String) {{ repository(owner: $owner, name: $name) {{ visibility pullRequests(states: OPEN, first: {PAGE}, after: $after, orderBy: {{field: UPDATED_AT, direction: DESC}}) {{ pageInfo {{ hasNextPage endCursor }} nodes {{ number title url isDraft createdAt updatedAt author {{ __typename login }} labels(first: {MOST_NAMED}) {{ nodes {{ name }} }} reviewDecision reviewRequests(first: 1) {{ totalCount }} mergeable mergeStateStatus isInMergeQueue headRefOid commits(last: 1) {{ nodes {{ commit {{ statusCheckRollup {{ state contexts(first: {MOST_NAMED}) {{ nodes {{ __typename ... on CheckRun {{ name conclusion }} ... on StatusContext {{ context state }} }} }} }} }} }} }} }} }} }} }}"
    )
}

/// The request for the page of the repository at `path` after `after`,
/// posted to `endpoint`, reading at most `parse` bytes.
pub(super) fn request(endpoint: &str, path: &str, after: Option<&str>, parse: usize) -> Request {
    let (owner, name) = path.split_once('/').unwrap_or((path, ""));
    let body = json!({
        "query": query(),
        "variables": { "owner": owner, "name": name, "after": after },
    });
    posted(endpoint, &body, parse)
}

#[derive(Deserialize)]
struct Answer {
    data: Option<Data>,
}

#[derive(Deserialize)]
struct Data {
    repository: Option<Repository>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Repository {
    visibility: Option<String>,
    pull_requests: Connection<Node>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection<T> {
    #[serde(default)]
    page_info: Option<PageInfo>,
    #[serde(default = "Vec::new")]
    nodes: Vec<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
struct Author {
    #[serde(rename = "__typename")]
    kind: String,
    login: String,
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Count {
    total_count: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Commit {
    commit: Head,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Head {
    status_check_rollup: Option<Rollup>,
}

#[derive(Deserialize)]
struct Rollup {
    state: String,
    contexts: Connection<Check>,
}

#[derive(Deserialize)]
struct Check {
    name: Option<String>,
    conclusion: Option<String>,
    context: Option<String>,
    state: Option<String>,
}

impl Check {
    /// Its name, where it failed.
    fn failed(self) -> Option<String> {
        let failed = self
            .conclusion
            .as_deref()
            .or(self.state.as_deref())
            .is_some_and(|state| FAILED.contains(&state));
        failed.then(|| self.name.or(self.context)).flatten()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Node {
    number: u64,
    title: String,
    url: String,
    is_draft: bool,
    created_at: Timestamp,
    updated_at: Timestamp,
    author: Option<Author>,
    labels: Option<Connection<Label>>,
    review_decision: Option<String>,
    review_requests: Option<Count>,
    mergeable: Option<String>,
    merge_state_status: Option<String>,
    #[serde(default)]
    is_in_merge_queue: bool,
    head_ref_oid: Option<String>,
    commits: Option<Connection<Commit>>,
}

/// What a rollup's `state` says of the checks, with the names of those
/// that failed.
fn checks(rollup: Option<Rollup>) -> Checks {
    let Some(rollup) = rollup else {
        return Checks::Absent;
    };
    match rollup.state.as_str() {
        "SUCCESS" => Checks::Passed,
        "FAILURE" | "ERROR" => Checks::Failed(
            rollup
                .contexts
                .nodes
                .into_iter()
                .filter_map(Check::failed)
                .collect(),
        ),
        _ => Checks::Pending,
    }
}

/// The turn GitHub's merge state status maps to.
fn forge_turn(status: &str) -> Turn {
    match status {
        "CLEAN" | "HAS_HOOKS" | "UNSTABLE" => Turn::WaitingOnMerge,
        "BLOCKED" => Turn::WaitingOnReview,
        "BEHIND" | "DIRTY" => Turn::WaitingOnAuthor,
        "DRAFT" => Turn::Draft,
        _ => Turn::Unknown,
    }
}

/// The visibility GitHub names.
fn visibility(named: Option<&str>) -> Visibility {
    match named {
        Some("PUBLIC") => Visibility::Public,
        Some("INTERNAL") => Visibility::Internal,
        _ => Visibility::Private,
    }
}

impl Node {
    /// The pull request, of `project` at `repository`, seen by `seen`.
    fn read(self, (project, repository): (&str, &str), seen: Visibility) -> Pull {
        let requested = self
            .review_requests
            .is_some_and(|requests| requests.total_count > 0);
        let review = match self.review_decision.as_deref() {
            Some("APPROVED") => Review::Approved,
            Some("CHANGES_REQUESTED") => Review::ChangesRequested,
            _ if requested => Review::Requested,
            _ => Review::Unasked,
        };
        let mergeable = match self.mergeable.as_deref() {
            Some("MERGEABLE") => Mergeable::Clean,
            Some("CONFLICTING") => Mergeable::Conflict,
            _ => Mergeable::Unknown,
        };
        let rollup = self
            .commits
            .and_then(|commits| commits.nodes.into_iter().next())
            .and_then(|commit| commit.commit.status_check_rollup);
        let bot = self
            .author
            .as_ref()
            .is_some_and(|author| author.kind == "Bot" || author.login.ends_with("[bot]"));
        let forge = if self.is_in_merge_queue {
            Some((Turn::Queued, "IN_MERGE_QUEUE".to_owned()))
        } else {
            self.merge_state_status
                .map(|status| (forge_turn(&status), status))
        };
        Pull {
            project: project.to_owned(),
            repository: repository.to_owned(),
            visibility: seen,
            number: self.number,
            title: self.title,
            url: self.url,
            author: self.author.map(|author| author.login),
            bot,
            draft: self.is_draft,
            labels: self
                .labels
                .map(|labels| labels.nodes.into_iter().map(|label| label.name).collect())
                .unwrap_or_default(),
            opened: self.created_at,
            updated: self.updated_at,
            review,
            checks: checks(rollup),
            mergeable,
            queued: self.is_in_merge_queue,
            forge,
            head: self.head_ref_oid,
            unread: false,
        }
    }
}

/// The page of open pull requests `body` holds, parsing no more than
/// `parse` bytes, named by `named`.
pub(super) fn page(body: &[u8], parse: usize, named: Named<'_>) -> Result<Page, SourceError> {
    let (project, repository, known) = named;
    let answer: Answer = herald_wire::read(WHAT, parse, body)?;
    let Some(found) = answer.data.and_then(|data| data.repository) else {
        return Err(SourceError::Unreadable {
            what: WHAT,
            reason: "it names no repository the token may read".to_owned(),
        });
    };
    let seen = known.unwrap_or_else(|| visibility(found.visibility.as_deref()));
    let next = found
        .pull_requests
        .page_info
        .filter(|info| info.has_next_page)
        .and_then(|info| info.end_cursor);
    let pulls = found
        .pull_requests
        .nodes
        .into_iter()
        .map(|node| node.read((project, repository), seen))
        .collect();
    Ok(Page { pulls, next })
}

#[cfg(test)]
mod tests;
