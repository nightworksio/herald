//! Forgejo Actions' runs.

use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;

use super::{Delivered, ForgeRun, Ran};
use crate::forge::Repository;
use crate::options::RunChoice;
use crate::wire::read;
use crate::{Conclusion, RunAction};

/// What Forgejo's run event is read as.
pub(super) const RUN: &str = "Forgejo's run event";
/// What Forgejo's listing of runs is read as.
pub(super) const RUNS: &str = "Forgejo's runs";
/// What a run with no workflow is called.
const UNNAMED: &str = "workflow";
/// The prefixes of the events Forgejo names a run's by: one per status in
/// newer releases, and one per finish in older ones.
const HOOKED: [&str; 2] = ["workflow_run_", "action_run_"];

/// Whether Forgejo's event `name` is of a run.
pub(super) fn hooked(name: &str) -> bool {
    HOOKED.iter().any(|prefix| {
        name.strip_prefix(prefix)
            .is_some_and(|status| !status.is_empty())
    })
}

/// What a run's status, as Forgejo names it, says it did and how it ended:
/// nothing for a status Forgejo does not name.
fn status(status: &str) -> Option<(RunAction, Option<Conclusion>)> {
    let done = |conclusion| Some((RunAction::Completed, Some(conclusion)));
    match status {
        "waiting" | "blocked" => Some((RunAction::Requested, None)),
        "running" => Some((RunAction::InProgress, None)),
        "success" => done(Conclusion::Success),
        "failure" => done(Conclusion::Failure),
        "cancelled" => done(Conclusion::Cancelled),
        "skipped" => done(Conclusion::Skipped),
        _ => None,
    }
}

/// Someone, by their login.
#[derive(Deserialize)]
struct User {
    login: String,
}

/// A repository, as Forgejo's run names it.
#[derive(Deserialize)]
struct Named {
    id: u64,
    name: String,
    full_name: String,
    html_url: Option<String>,
}

/// A run, as Forgejo describes it.
#[derive(Deserialize)]
struct ActionRun {
    id: u64,
    index_in_repo: Option<u64>,
    workflow_id: Option<String>,
    title: Option<String>,
    status: String,
    event: Option<String>,
    prettyref: Option<String>,
    commit_sha: Option<String>,
    html_url: String,
    stopped: Option<Timestamp>,
    updated: Timestamp,
    trigger_user: Option<User>,
    repository: Option<Named>,
}

impl ActionRun {
    fn run(self) -> Option<ForgeRun> {
        let (action, conclusion) = status(&self.status)?;
        let occurred = match action {
            RunAction::Completed => self.stopped.unwrap_or(self.updated),
            RunAction::Requested | RunAction::InProgress => self.updated,
        };
        Some(ForgeRun {
            ran: Ran::Run,
            id: self.id,
            attempt: None,
            number: self.index_in_repo,
            workflow: self.workflow_id.unwrap_or_else(|| UNNAMED.to_owned()),
            action,
            conclusion,
            trigger: self.event,
            branch: self.prettyref,
            commit: self.commit_sha,
            actor: self.trigger_user.map(|user| user.login),
            url: self.html_url,
            title: self.title,
            occurred,
        })
    }
}

/// Forgejo's run event, of either generation.
#[derive(Deserialize)]
struct Hook {
    run: ActionRun,
}

/// What Forgejo's run event `payload` names.
pub(super) fn delivered(payload: &[u8]) -> Result<Delivered, SourceError> {
    let mut hook: Hook = read(RUN, payload)?;
    let named = hook
        .run
        .repository
        .take()
        .ok_or_else(|| SourceError::Unreadable {
            what: RUN,
            reason: "the run names no repository".to_owned(),
        })?;
    Ok(Delivered {
        path: named.full_name,
        repository: Repository {
            id: named.id,
            name: named.name,
            page: named.html_url,
        },
        run: hook.run.run(),
    })
}

/// Forgejo's listing of runs.
#[derive(Deserialize)]
struct Runs {
    workflow_runs: Vec<ActionRun>,
}

/// The runs in Forgejo's listing `body`, parsing no more than `limit`
/// bytes.
pub(super) fn listed(limit: usize, body: &[u8]) -> Result<Vec<ForgeRun>, SourceError> {
    let listed: Runs = herald_wire::read(RUNS, limit, body)?;
    Ok(listed
        .workflow_runs
        .into_iter()
        .filter_map(ActionRun::run)
        .collect())
}

/// The path, under a repository, of its newest `page` runs, asking Forgejo
/// for only those `choice` announces where one parameter can say so.
pub(super) fn runs_path(choice: &RunChoice, page: usize) -> String {
    let path = format!("/actions/runs?limit={page}");
    let status = match (choice.only_action(), choice.only_conclusion()) {
        (Some(RunAction::Completed), Some(conclusion)) => Some(conclusion.name()),
        _ => None,
    };
    let reference = choice
        .only_branch()
        .map(|branch| format!("refs/heads/{branch}"));
    super::queried(
        path,
        &[
            ("status", status),
            ("event", choice.only_trigger()),
            ("ref", reference.as_deref()),
        ],
    )
}
