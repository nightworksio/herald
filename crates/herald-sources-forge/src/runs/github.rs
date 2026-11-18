//! GitHub's workflow runs, check suites and commit statuses.

use herald_core::{unreserved, web_url};
use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;

use super::{Delivered, ForgeRun, Ran, github_action};
use crate::forge::Repository;
use crate::options::RunChoice;
use crate::wire::read;
use crate::{Conclusion, RunAction};

/// What GitHub's `workflow_run` event is read as.
pub(super) const RUN: &str = "GitHub's workflow run event";
/// What GitHub's listing of workflow runs is read as.
pub(super) const RUNS: &str = "GitHub's workflow runs";
/// What GitHub's `check_suite` event is read as.
const SUITE: &str = "GitHub's check suite event";
/// What GitHub's listing of a commit's check suites is read as.
pub(super) const SUITES: &str = "GitHub's check suites";
/// What GitHub's `status` event is read as.
const STATUS: &str = "GitHub's commit status event";
/// What GitHub's listing of a commit's statuses is read as.
pub(super) const STATUSES: &str = "GitHub's commit statuses";
/// The reference a poll of checks reads where no one branch is chosen: the
/// default branch's newest commit.
pub(super) const HEAD: &str = "HEAD";
/// The app whose check suites are GitHub Actions' own, read as workflow
/// runs.
const ACTIONS_APP: &str = "github-actions";
/// What a workflow run with no name is called.
const UNNAMED_RUN: &str = "workflow";
/// What a commit status with no context is called.
const UNNAMED_STATUS: &str = "status";

/// Someone, by their login.
#[derive(Deserialize)]
struct Login {
    login: String,
}

/// A workflow run, as GitHub describes it.
#[derive(Deserialize)]
struct WorkflowRun {
    id: u64,
    name: Option<String>,
    run_number: Option<u64>,
    run_attempt: Option<u64>,
    event: Option<String>,
    status: Option<String>,
    conclusion: Option<String>,
    head_branch: Option<String>,
    head_sha: Option<String>,
    html_url: String,
    display_title: Option<String>,
    updated_at: Timestamp,
    actor: Option<Login>,
}

impl From<WorkflowRun> for ForgeRun {
    fn from(run: WorkflowRun) -> Self {
        Self {
            ran: Ran::Run,
            id: run.id,
            attempt: run.run_attempt,
            number: run.run_number,
            workflow: run.name.unwrap_or_else(|| UNNAMED_RUN.to_owned()),
            action: github_action(run.status.as_deref().unwrap_or_default()),
            conclusion: run.conclusion.as_deref().and_then(Conclusion::named),
            trigger: run.event,
            branch: run.head_branch,
            commit: run.head_sha,
            actor: run.actor.map(|actor| actor.login),
            url: run.html_url,
            title: run.display_title,
            occurred: run.updated_at,
        }
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

impl Named {
    fn split(self) -> (String, Repository) {
        (
            self.full_name,
            Repository {
                id: self.id,
                name: self.name,
                page: self.html_url,
            },
        )
    }
}

/// GitHub's `workflow_run` event.
#[derive(Deserialize)]
struct RunEvent {
    workflow_run: WorkflowRun,
    repository: Named,
}

/// What GitHub's `workflow_run` event `payload` names.
pub(super) fn delivered_run(payload: &[u8]) -> Result<Delivered, SourceError> {
    let event: RunEvent = read(RUN, payload)?;
    let (path, repository) = event.repository.split();
    Ok(Delivered {
        path,
        repository,
        run: Some(event.workflow_run.into()),
    })
}

/// GitHub's listing of workflow runs.
#[derive(Deserialize)]
struct WorkflowRuns {
    workflow_runs: Vec<WorkflowRun>,
}

/// The workflow runs in GitHub's listing `body`, parsing no more than
/// `limit` bytes.
pub(super) fn listed_runs(limit: usize, body: &[u8]) -> Result<Vec<ForgeRun>, SourceError> {
    let listed: WorkflowRuns = herald_wire::read(RUNS, limit, body)?;
    Ok(listed
        .workflow_runs
        .into_iter()
        .map(ForgeRun::from)
        .collect())
}

/// The path, under a repository, of its newest `page` workflow runs, asking
/// GitHub for only those `choice` announces where one parameter can say so.
pub(super) fn runs_path(choice: &RunChoice, page: usize) -> String {
    let path = format!("/actions/runs?per_page={page}");
    let status = match (choice.only_action(), choice.only_conclusion()) {
        (Some(RunAction::Completed), Some(conclusion)) => Some(conclusion.name()),
        (Some(action), _) => Some(action.name()),
        (None, _) => None,
    };
    let asked = [
        ("status", status),
        ("event", choice.only_trigger()),
        ("branch", choice.only_branch()),
    ];
    super::queried(path, &asked)
}

/// The app a check suite is of.
#[derive(Deserialize)]
struct App {
    slug: String,
    name: String,
}

/// A check suite, as GitHub describes it.
#[derive(Deserialize)]
struct CheckSuite {
    id: u64,
    head_branch: Option<String>,
    head_sha: String,
    status: Option<String>,
    conclusion: Option<String>,
    app: App,
    updated_at: Timestamp,
}

/// The run `suite` of the repository whose page is `page` is, where it is
/// not GitHub Actions' own.
fn suite_run(suite: CheckSuite, page: &str) -> Option<ForgeRun> {
    (suite.app.slug != ACTIONS_APP).then(|| ForgeRun {
        ran: Ran::CheckSuite,
        id: suite.id,
        attempt: None,
        number: None,
        workflow: suite.app.name,
        action: github_action(suite.status.as_deref().unwrap_or_default()),
        conclusion: suite.conclusion.as_deref().and_then(Conclusion::named),
        trigger: None,
        branch: suite.head_branch,
        url: format!(
            "{page}/commit/{}/checks?check_suite_id={}",
            unreserved(&suite.head_sha),
            suite.id
        ),
        commit: Some(suite.head_sha),
        actor: None,
        title: None,
        occurred: suite.updated_at,
    })
}

/// The web page of `repository`, which a check suite's page is under.
fn page_of(what: &'static str, repository: &Repository) -> Result<String, SourceError> {
    repository
        .page
        .clone()
        .ok_or_else(|| SourceError::Unreadable {
            what,
            reason: "it names no web page of the repository".to_owned(),
        })
}

/// GitHub's `check_suite` event.
#[derive(Deserialize)]
struct SuiteEvent {
    check_suite: CheckSuite,
    repository: Named,
}

/// What GitHub's `check_suite` event `payload` names.
pub(super) fn delivered_suite(payload: &[u8]) -> Result<Delivered, SourceError> {
    let event: SuiteEvent = read(SUITE, payload)?;
    let (path, repository) = event.repository.split();
    let page = page_of(SUITE, &repository)?;
    Ok(Delivered {
        path,
        repository,
        run: suite_run(event.check_suite, &page),
    })
}

/// GitHub's listing of a commit's check suites.
#[derive(Deserialize)]
struct CheckSuites {
    check_suites: Vec<CheckSuite>,
}

/// The check suites of `repository` in GitHub's listing `body`, parsing no
/// more than `limit` bytes.
pub(super) fn listed_suites(
    limit: usize,
    repository: &Repository,
    body: &[u8],
) -> Result<Vec<ForgeRun>, SourceError> {
    let listed: CheckSuites = herald_wire::read(SUITES, limit, body)?;
    let page = page_of(SUITES, repository)?;
    Ok(listed
        .check_suites
        .into_iter()
        .filter_map(|suite| suite_run(suite, &page))
        .collect())
}

/// The paths, under a repository, of the newest `page` check suites and
/// statuses of the commit `reference` names, and what each lists.
pub(super) fn checks_paths(reference: &str, page: usize) -> [(String, Ran); 2] {
    let reference = crate::forge::segments(reference);
    [
        (
            format!("/commits/{reference}/check-suites?per_page={page}"),
            Ran::CheckSuite,
        ),
        (
            format!("/commits/{reference}/statuses?per_page={page}"),
            Ran::Status,
        ),
    ]
}

/// A branch a status event names.
#[derive(Deserialize)]
struct Branch {
    name: String,
}

/// A commit status, as GitHub's event or listing describes it.
#[derive(Deserialize)]
struct Status {
    id: u64,
    state: String,
    context: Option<String>,
    description: Option<String>,
    target_url: Option<String>,
    updated_at: Timestamp,
    #[serde(default)]
    sha: Option<String>,
    #[serde(default)]
    branches: Vec<Branch>,
    #[serde(default, alias = "creator")]
    sender: Option<Login>,
}

/// The run the status `status`, on `branch` where its event names none, of
/// the repository whose page is `page` is: its target where that is a web
/// page, or else its commit's, or the repository's.
fn status_run(status: Status, (page, branch): (&str, Option<String>)) -> ForgeRun {
    let (action, conclusion) = match status.state.as_str() {
        "success" => (RunAction::Completed, Some(Conclusion::Success)),
        "failure" | "error" => (RunAction::Completed, Some(Conclusion::Failure)),
        _ => (RunAction::InProgress, None),
    };
    let commit = status.sha;
    let url = status
        .target_url
        .filter(|url| web_url(url))
        .unwrap_or_else(|| match &commit {
            Some(sha) => format!("{page}/commit/{}", unreserved(sha)),
            None => page.to_owned(),
        });
    ForgeRun {
        ran: Ran::Status,
        id: status.id,
        attempt: None,
        number: None,
        workflow: status.context.unwrap_or_else(|| UNNAMED_STATUS.to_owned()),
        action,
        conclusion,
        trigger: None,
        branch: status
            .branches
            .into_iter()
            .next()
            .map(|branch| branch.name)
            .or(branch),
        commit,
        actor: status.sender.map(|sender| sender.login),
        url,
        title: status.description,
        occurred: status.updated_at,
    }
}

/// GitHub's `status` event.
#[derive(Deserialize)]
struct StatusEvent {
    #[serde(flatten)]
    status: Status,
    repository: Named,
}

/// What GitHub's `status` event `payload` names.
pub(super) fn delivered_status(payload: &[u8]) -> Result<Delivered, SourceError> {
    let event: StatusEvent = read(STATUS, payload)?;
    let (path, repository) = event.repository.split();
    let page = page_of(STATUS, &repository)?;
    Ok(Delivered {
        path,
        run: Some(status_run(event.status, (&page, None))),
        repository,
    })
}

/// The statuses of `repository`'s commit `reference` names, on that branch
/// where it names one, in GitHub's listing `body`, parsing no more than
/// `limit` bytes.
pub(super) fn listed_statuses(
    limit: usize,
    (repository, reference): (&Repository, Option<String>),
    body: &[u8],
) -> Result<Vec<ForgeRun>, SourceError> {
    let listed: Vec<Status> = herald_wire::read(STATUSES, limit, body)?;
    let page = page_of(STATUSES, repository)?;
    Ok(listed
        .into_iter()
        .map(|status| status_run(status, (&page, reference.clone())))
        .collect())
}
