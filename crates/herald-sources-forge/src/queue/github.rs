use std::fmt::Write as _;

use super::Titled;
use herald_core::{Changed, Checks, Mergeable, Pull, QueueEntry, Review, Turn, Visibility};
use herald_ports::Request;
use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;

use crate::github::Label;
use serde_json::json;

use super::{FAILED, Named, PAGE, Page};
use crate::graphql::posted;

/// What GitHub's open pull requests are read as.
pub(super) const WHAT: &str = "GitHub's open pull requests";

/// The most labels and check contexts a pull request is read with.
const MOST_NAMED: usize = 50;

/// The most paths a pull request's head changes it is read with, where a
/// view predicts conflicts (ADR-0063).
pub(super) const MOST_FILES: usize = 100;

/// What the query reads of each pull request's merge queue entry, where
/// a view reads the queue (ADR-0078).
const ENTRY: &str = " mergeQueueEntry { position state enqueuedAt estimatedTimeToMerge }";

/// How a request reads its answer: at most `parse` bytes, each pull
/// request's changed paths too where `files` says (ADR-0063), and its
/// merge queue entry where `queue` says (ADR-0078).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Ask {
    pub(super) parse: usize,
    pub(super) files: bool,
    pub(super) queue: bool,
}

/// What the query reads of each pull request, as `ask` says.
fn fields(ask: Ask) -> String {
    let changed = if ask.files {
        format!(" files(first: {MOST_FILES}) {{ totalCount nodes {{ path }} }}")
    } else {
        String::new()
    };
    let entry = if ask.queue { ENTRY } else { "" };
    format!(
        "number title url isDraft createdAt updatedAt baseRefName{changed} author {{ __typename login }} labels(first: {MOST_NAMED}) {{ nodes {{ name }} }} reviewDecision reviewRequests(first: {MOST_NAMED}) {{ totalCount nodes {{ requestedReviewer {{ ... on User {{ login }} ... on Bot {{ login }} ... on Mannequin {{ login }} ... on Team {{ combinedSlug }} }} }} }} mergeable mergeStateStatus isInMergeQueue{entry} headRefOid headRefName milestone {{ title }} commits(last: 1) {{ nodes {{ commit {{ statusCheckRollup {{ state contexts(first: {MOST_NAMED}) {{ nodes {{ __typename ... on CheckRun {{ name conclusion }} ... on StatusContext {{ context state }} }} }} }} }} }} }}"
    )
}

/// The query for one page of a repository's open pull requests, the most
/// recently updated first.
fn query(ask: Ask) -> String {
    format!(
        "query($owner: String!, $name: String!, $after: String) {{ repository(owner: $owner, name: $name) {{ visibility pullRequests(states: OPEN, first: {PAGE}, after: $after, orderBy: {{field: UPDATED_AT, direction: DESC}}) {{ pageInfo {{ hasNextPage endCursor }} nodes {{ {} }} }} }} }}",
        fields(ask)
    )
}

/// The query reading each of a repository's pull requests `numbers` in
/// full, under an alias holding its place.
fn numbered_query(numbers: &[u64], ask: Ask) -> String {
    let fields = fields(ask);
    let mut pulls = String::new();
    for (place, number) in numbers.iter().enumerate() {
        let _ = write!(
            pulls,
            " p{place}: pullRequest(number: {number}) {{ {fields} }}"
        );
    }
    format!(
        "query($owner: String!, $name: String!) {{ repository(owner: $owner, name: $name) {{ visibility{pulls} }} }}"
    )
}

/// The query reading in full each of the pull requests of each repository
/// of `groups`, a repository under the alias `r<place>` and each of its
/// pull requests under `p<place>` (ADR-0062).
fn scoped_query(groups: &[(&str, Vec<u64>)], ask: Ask) -> String {
    let fields = fields(ask);
    let mut declared = Vec::new();
    let mut repositories = String::new();
    for (place, (_, numbers)) in groups.iter().enumerate() {
        declared.push(format!("$o{place}: String!, $n{place}: String!"));
        let _ = write!(
            repositories,
            " r{place}: repository(owner: $o{place}, name: $n{place}) {{ visibility"
        );
        for (pull, number) in numbers.iter().enumerate() {
            let _ = write!(
                repositories,
                " p{pull}: pullRequest(number: {number}) {{ {fields} }}"
            );
        }
        repositories.push_str(" }");
    }
    format!("query({}) {{{repositories} }}", declared.join(", "))
}

/// The request reading in full each pull request of each repository of
/// `groups`, each a repository's full path and its pull requests' numbers,
/// posted to `endpoint`, read as `ask` says (ADR-0062).
pub(super) fn scoped_request(endpoint: &str, groups: &[(&str, Vec<u64>)], ask: Ask) -> Request {
    let mut variables = serde_json::Map::new();
    for (place, (path, _)) in groups.iter().enumerate() {
        let (owner, name) = path.split_once('/').unwrap_or((path, ""));
        variables.insert(format!("o{place}"), json!(owner));
        variables.insert(format!("n{place}"), json!(name));
    }
    let body = json!({ "query": scoped_query(groups, ask), "variables": variables });
    posted(endpoint, &body, ask.parse)
}

/// The request for the page of the repository at `path` after `after`,
/// posted to `endpoint`, read as `ask` says.
pub(super) fn request(endpoint: &str, path: &str, after: Option<&str>, ask: Ask) -> Request {
    let (owner, name) = path.split_once('/').unwrap_or((path, ""));
    let body = json!({
        "query": query(ask),
        "variables": { "owner": owner, "name": name, "after": after },
    });
    posted(endpoint, &body, ask.parse)
}

/// The request reading each of the pull requests `numbers` of the
/// repository at `path` in full, posted to `endpoint`, read as `ask`
/// says.
pub(super) fn numbered_request(endpoint: &str, path: &str, numbers: &[u64], ask: Ask) -> Request {
    let (owner, name) = path.split_once('/').unwrap_or((path, ""));
    let body = json!({
        "query": numbered_query(numbers, ask),
        "variables": { "owner": owner, "name": name },
    });
    posted(endpoint, &body, ask.parse)
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
struct NumberedAnswer {
    data: Option<NumberedData>,
}

#[derive(Deserialize)]
struct NumberedData {
    repository: Option<Numbered>,
}

#[derive(Deserialize)]
struct Numbered {
    visibility: Option<String>,
    #[serde(flatten)]
    pulls: std::collections::BTreeMap<String, Option<Node>>,
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
#[serde(rename_all = "camelCase")]
struct Requests {
    total_count: u64,
    #[serde(default)]
    nodes: Vec<Asked>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Asked {
    requested_reviewer: Option<Reviewer>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reviewer {
    login: Option<String>,
    combined_slug: Option<String>,
}

impl Requests {
    /// The login of each reviewer asked, or a team's `org/team`.
    fn reviewers(self) -> Vec<String> {
        self.nodes
            .into_iter()
            .filter_map(|request| request.requested_reviewer)
            .filter_map(|reviewer| reviewer.login.or(reviewer.combined_slug))
            .collect()
    }
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
    review_requests: Option<Requests>,
    mergeable: Option<String>,
    merge_state_status: Option<String>,
    #[serde(default)]
    is_in_merge_queue: bool,
    merge_queue_entry: Option<Entry>,
    head_ref_oid: Option<String>,
    head_ref_name: Option<String>,
    milestone: Option<Titled>,
    commits: Option<Connection<Commit>>,
    base_ref_name: Option<String>,
    files: Option<Files>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    position: u64,
    state: String,
    enqueued_at: Timestamp,
    estimated_time_to_merge: Option<u64>,
}

impl Entry {
    /// The entry as herald names it.
    fn read(self) -> QueueEntry {
        QueueEntry {
            position: Some(self.position),
            state: super::merging::state(&self.state),
            since: Some(self.enqueued_at),
            estimate: self.estimated_time_to_merge,
            left: None,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Files {
    total_count: usize,
    #[serde(default)]
    nodes: Vec<Path>,
}

#[derive(Deserialize)]
struct Path {
    path: String,
}

impl Files {
    /// The paths it names, whole where it names every one.
    fn changed(self) -> Changed {
        let whole = self.nodes.len() >= self.total_count;
        Changed {
            paths: self.nodes.into_iter().map(|file| file.path).collect(),
            whole,
        }
    }
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
            .as_ref()
            .is_some_and(|requests| requests.total_count > 0);
        let reviewers = self
            .review_requests
            .map(Requests::reviewers)
            .unwrap_or_default();
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
            .is_some_and(|author| crate::github::bot(Some(&author.kind), &author.login));
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
            reviewers,
            checks: checks(rollup),
            mergeable,
            queue: self
                .merge_queue_entry
                .map(Entry::read)
                .or_else(|| self.is_in_merge_queue.then_some(QueueEntry::entered(None))),
            forge,
            head: self.head_ref_oid,
            unread: false,
            base: self.base_ref_name,
            files: self.files.map(Files::changed),
            areas: Vec::new(),
            head_branch: self.head_ref_name,
            milestone: self.milestone.map(|milestone| milestone.title),
            flips: Vec::new(),
        }
    }
}

/// The page of open pull requests `body` holds, parsing no more than
/// `parse` bytes, named by `named`.
pub(super) fn page(body: &[u8], parse: usize, named: Named<'_>) -> Result<Page, SourceError> {
    let (project, repository, known) = named;
    let answer: Answer = herald_wire::read(WHAT, parse, body)?;
    let Some(found) = answer.data.and_then(|data| data.repository) else {
        return Err(unnamed());
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
    Ok(Page {
        pulls,
        next,
        repository: None,
        visibility: None,
    })
}

/// The pull requests `body` reads in full, parsing no more than `parse`
/// bytes, named by `named`; one the answer holds no pull request for is
/// left out.
pub(super) fn numbered(
    body: &[u8],
    parse: usize,
    named: Named<'_>,
) -> Result<Vec<Pull>, SourceError> {
    let (project, repository, known) = named;
    let answer: NumberedAnswer = herald_wire::read(WHAT, parse, body)?;
    let Some(found) = answer.data.and_then(|data| data.repository) else {
        return Err(unnamed());
    };
    let seen = known.unwrap_or_else(|| visibility(found.visibility.as_deref()));
    Ok(found
        .pulls
        .into_values()
        .flatten()
        .map(|node| node.read((project, repository), seen))
        .collect())
}

#[derive(Deserialize)]
struct ScopedAnswer {
    data: Option<std::collections::BTreeMap<String, Option<Numbered>>>,
}

/// The pull requests `body` reads in full of each repository `named`
/// names in the order [`scoped_request`] asked them, parsing no more than
/// `parse` bytes, each seen by the stricter of who its repository's own
/// answer and `named` say may see it; a repository or pull request the
/// answer holds nothing for is left out (ADR-0062).
pub(super) fn scoped(
    body: &[u8],
    parse: usize,
    named: &[Named<'_>],
) -> Result<Vec<Pull>, SourceError> {
    let answer: ScopedAnswer = herald_wire::read(WHAT, parse, body)?;
    let mut found = answer.data.unwrap_or_default();
    let mut pulls = Vec::new();
    for (place, (project, repository, known)) in named.iter().enumerate() {
        let Some(Some(numbered)) = found.remove(&format!("r{place}")) else {
            continue;
        };
        let read = visibility(numbered.visibility.as_deref());
        let seen = known.map_or(read, |known| known.max(read));
        pulls.extend(
            numbered
                .pulls
                .into_values()
                .flatten()
                .map(|node| node.read((project, repository), seen)),
        );
    }
    Ok(pulls)
}

/// Why an answer naming no repository cannot be read.
fn unnamed() -> SourceError {
    SourceError::Unreadable {
        what: WHAT,
        reason: "it names no repository the token may read".to_owned(),
    }
}

#[cfg(test)]
mod tests;
