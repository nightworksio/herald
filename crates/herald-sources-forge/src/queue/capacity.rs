//! A view's capacity overview: each repository's CI runs queued and
//! running, read as its forge lists them, and what could not be read with
//! why (ADR-0066).

use herald_core::{Overviews, Running, Unread, Visibility};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Http as _};
use herald_wire::answered;
use jiff::Timestamp;
use serde::Deserialize;

use super::{Opened, PAGE};
use crate::Forge;
use crate::limited::limited;

/// What a listing that filled its page says of the runs it counts.
pub const MORE: &str = "a listing held more than";

/// The most tasks a page of Forgejo's Actions tasks holds.
const FORGEJO_PAGE: usize = 50;

/// What a repository's queued or running workflow runs are read as.
const RUNS: &str = "a repository's queued and running workflow runs";

/// What a repository's pending or running pipelines are read as.
const PIPELINES: &str = "a repository's pending and running pipelines";

/// What a repository's Forgejo Actions tasks are read as.
const TASKS: &str = "a repository's Forgejo Actions tasks";

/// Why a GitHub repository's runs could not be read where it refused.
pub const GITHUB_REFUSED: &str = "the token may not read its Actions runs: a GitHub App or fine-grained token needs Actions: read, a classic token repo (public_repo for a public repository)";

/// Why a GitLab project's pipelines could not be read where it refused.
pub const GITLAB_REFUSED: &str =
    "the token may not read its pipelines: it needs read_api and the Reporter role or higher";

/// Why a Forgejo repository's tasks could not be read where it refused.
pub const FORGEJO_REFUSED: &str =
    "the token may not read its Actions tasks: it needs read access to the repository";

/// Why a Forgejo repository's tasks could not be read where its instance
/// has no API for them.
pub const FORGEJO_UNLISTED: &str =
    "this Forgejo answers no Actions tasks API, so its runs are not counted";

/// Why a repository's runs could not be read where the forge found none of
/// it.
pub const UNSEEN: &str = "the forge found no such repository, or the token may not see it";

#[derive(Deserialize)]
struct Runs {
    #[serde(default)]
    workflow_runs: Vec<Run>,
}

#[derive(Deserialize)]
struct Run {
    id: u64,
    html_url: String,
    head_sha: String,
    event: String,
    created_at: Timestamp,
}

#[derive(Deserialize)]
struct Pipeline {
    id: u64,
    web_url: String,
    sha: String,
    source: Option<String>,
    created_at: Timestamp,
}

#[derive(Deserialize)]
struct Tasks {
    #[serde(default)]
    workflow_runs: Vec<Task>,
}

#[derive(Deserialize)]
struct Task {
    id: u64,
    url: String,
    head_sha: String,
    event: String,
    status: String,
    created_at: Timestamp,
}

/// Whether a run started by `event` was started for a pull request.
fn pulled(event: &str) -> bool {
    event.starts_with("pull_request") || event == "merge_request_event"
}

impl Opened<'_> {
    /// What its forge's listing of runs is read as.
    const fn runs_what(&self) -> &'static str {
        match self.origin.forge() {
            Forge::GitHub => RUNS,
            Forge::GitLab => PIPELINES,
            Forge::Forgejo => TASKS,
        }
    }

    /// The runs its forge's listing `body` holds, each queued where
    /// `queued` says or its state does, seen as `visibility`, and how many
    /// the listing held.
    fn listed_runs(
        &self,
        body: &[u8],
        queued: bool,
        visibility: Visibility,
    ) -> Result<(Vec<Running>, usize), Unpolled> {
        let parse = self.origin.reading().parse;
        let path = self.origin.path();
        let running = |(id, url, sha, pull, created): (u64, String, String, bool, Timestamp),
                       queued: bool| Running {
            repository: path.to_owned(),
            visibility,
            id: id.to_string(),
            url,
            sha,
            pull,
            queued,
            created,
        };
        Ok(match self.origin.forge() {
            Forge::GitHub => {
                let listed: Runs =
                    herald_wire::read(RUNS, parse, body).map_err(Unpolled::unreadable)?;
                let held = listed.workflow_runs.len();
                let runs = listed
                    .workflow_runs
                    .into_iter()
                    .map(|run| {
                        let pull = pulled(&run.event);
                        running(
                            (run.id, run.html_url, run.head_sha, pull, run.created_at),
                            queued,
                        )
                    })
                    .collect();
                (runs, held)
            }
            Forge::GitLab => {
                let listed: Vec<Pipeline> =
                    herald_wire::read(PIPELINES, parse, body).map_err(Unpolled::unreadable)?;
                let held = listed.len();
                let runs = listed
                    .into_iter()
                    .map(|pipeline| {
                        let pull = pipeline.source.as_deref().is_some_and(pulled);
                        let read = (
                            pipeline.id,
                            pipeline.web_url,
                            pipeline.sha,
                            pull,
                            pipeline.created_at,
                        );
                        running(read, queued)
                    })
                    .collect();
                (runs, held)
            }
            Forge::Forgejo => {
                let listed: Tasks =
                    herald_wire::read(TASKS, parse, body).map_err(Unpolled::unreadable)?;
                let held = listed.workflow_runs.len();
                let runs = listed
                    .workflow_runs
                    .into_iter()
                    .filter(|task| matches!(task.status.as_str(), "waiting" | "running"))
                    .map(|task| {
                        let queued = task.status == "waiting";
                        let pull = pulled(&task.event);
                        running(
                            (task.id, task.url, task.head_sha, pull, task.created_at),
                            queued,
                        )
                    })
                    .collect();
                (runs, held)
            }
        })
    }
}

impl Opened<'_> {
    /// The CI runs of its repository queued and running, seen as
    /// `visibility`, asked through `http`: each run a listing holds, a
    /// listing that filled its page named as holding more than it counts,
    /// and the repository named unread with why where the forge would not
    /// list them, never counted as idle.
    ///
    /// # Errors
    ///
    /// Where a listing could not be asked, or answered what is no listing.
    pub async fn capacity(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        visibility: Visibility,
    ) -> Result<Overviews, Unpolled> {
        let path = self.origin.path();
        let unread = |reason: String| Unread {
            repository: path.to_owned(),
            visibility,
            reason,
        };
        let listings: Vec<(String, bool, usize)> = match self.origin.forge() {
            Forge::GitHub => vec![
                (
                    format!("/actions/runs?status=queued&per_page={PAGE}"),
                    true,
                    PAGE,
                ),
                (
                    format!("/actions/runs?status=in_progress&per_page={PAGE}"),
                    false,
                    PAGE,
                ),
            ],
            Forge::GitLab => vec![
                (
                    format!("/pipelines?status=pending&per_page={PAGE}"),
                    true,
                    PAGE,
                ),
                (
                    format!("/pipelines?status=running&per_page={PAGE}"),
                    false,
                    PAGE,
                ),
            ],
            Forge::Forgejo => vec![(
                format!("/actions/tasks?limit={FORGEJO_PAGE}"),
                false,
                FORGEJO_PAGE,
            )],
        };
        let mut overviews = Overviews::default();
        for (listing, queued, page) in &listings {
            let asked = self.origin.get(listing, self.token)?;
            let answer = http.send(asked).await.map_err(Unpolled::unreachable)?;
            if let Some(limited) = limited(self.origin.forge(), self.runs_what(), &answer) {
                return Err(limited);
            }
            let refused = match (self.origin.forge(), answer.status) {
                (Forge::Forgejo, 404) => Some(FORGEJO_UNLISTED),
                (_, 404) => Some(UNSEEN),
                (Forge::GitHub, 401 | 403) => Some(GITHUB_REFUSED),
                (Forge::GitLab, 401 | 403) => Some(GITLAB_REFUSED),
                (Forge::Forgejo, 401 | 403) => Some(FORGEJO_REFUSED),
                _ => None,
            };
            if let Some(reason) = refused {
                return Ok(Overviews {
                    capacity: Some(Vec::new()),
                    unread: vec![unread(reason.to_owned())],
                    ..Overviews::default()
                });
            }
            answered(self.runs_what(), &answer).map_err(Unpolled::unreadable)?;
            let (runs, listed) = self.listed_runs(&answer.body, *queued, visibility)?;
            if listed >= *page {
                overviews.unread.push(unread(format!(
                    "{MORE} {page} of its runs; only those {page} are counted"
                )));
            }
            overviews.capacity.get_or_insert_default().extend(runs);
        }
        Ok(overviews)
    }
}

/// The overview `all`, each repository's read, holds: every run counted,
/// and every repository that could not be read, or wholly, named with why.
pub(crate) fn counted(all: Vec<Overviews>) -> Overviews {
    let mut overviews = Overviews {
        capacity: Some(Vec::new()),
        ..Overviews::default()
    };
    for read in all {
        overviews.merge(read);
    }
    overviews
}

#[cfg(test)]
mod tests;
