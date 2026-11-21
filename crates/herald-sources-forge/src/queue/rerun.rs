//! A rule's plan to re-run a flaky check: the failed runs on a pull
//! request's head that ran it, each with the exact call that re-runs its
//! failed jobs (ADR-0065).

use herald_core::Rerun;
use herald_kinds::{Plan, Unpolled};
use herald_ports::{DynHttp, Erased};
use serde::Deserialize;

use super::{Opened, PAGE, failed};
use crate::Forge;

/// What a head's failed GitHub Actions runs are read as.
const RUNS: &str = "a head's failed workflow runs";

/// What a run's jobs are read as.
const JOBS: &str = "a workflow run's jobs";

/// What a head's failed GitLab pipelines are read as.
const PIPELINES: &str = "a head's failed pipelines";

/// Why a check no failed GitHub Actions run ran cannot be re-run.
pub const NO_RUN: &str =
    "no failed GitHub Actions run on the head ran this check, so herald has nothing to re-run";

/// Why a GitLab merge request with no failed pipeline cannot be re-run.
pub const NO_PIPELINE: &str = "no failed pipeline ran on the head, so herald has nothing to re-run";

/// Why nothing on Forgejo can be re-run.
pub const UNRERUN: &str = "Forgejo has no API to re-run a run, so re-run it by hand";

#[derive(Deserialize)]
struct Runs {
    #[serde(default)]
    workflow_runs: Vec<Run>,
}

#[derive(Deserialize)]
struct Run {
    id: u64,
    html_url: String,
}

#[derive(Deserialize)]
struct Jobs {
    #[serde(default)]
    jobs: Vec<Job>,
}

#[derive(Deserialize)]
struct Job {
    name: String,
    conclusion: Option<String>,
}

#[derive(Deserialize)]
struct Pipeline {
    id: u64,
    web_url: String,
}

impl Opened<'_> {
    /// The failed runs on `head` that ran the check `check`, the newest
    /// `most` read, each with the exact call re-running its failed jobs,
    /// asked through `http`: GitHub Actions' runs whose job of that name
    /// failed, GitLab's failed pipelines, and nothing on Forgejo, which
    /// says why.
    ///
    /// # Errors
    ///
    /// Where the runs could not be asked or read.
    pub async fn planned(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (head, check): (&str, &str),
        most: usize,
    ) -> Result<Plan, Unpolled> {
        let parse = self.origin.reading().parse;
        let (api, path) = (self.origin.api(), self.origin.path());
        let runs = match self.origin.forge() {
            Forge::Forgejo => return Ok(Plan::Unplanned(UNRERUN)),
            Forge::GitLab => {
                let asked = self.origin.get(
                    &format!("/pipelines?sha={head}&status=failed&per_page={most}"),
                    self.token,
                );
                let body = self.asked(http, asked, PIPELINES).await?;
                let pipelines: Vec<Pipeline> =
                    herald_wire::read(PIPELINES, parse, &body).map_err(Unpolled::unreadable)?;
                let project = crate::gitlab::project(api, path);
                pipelines
                    .into_iter()
                    .take(most)
                    .map(|pipeline| Rerun {
                        run: pipeline.id.to_string(),
                        page: pipeline.web_url,
                        call: format!("POST {project}/pipelines/{}/retry", pipeline.id),
                    })
                    .collect::<Vec<_>>()
            }
            Forge::GitHub => {
                let asked = self.origin.get(
                    &format!("/actions/runs?head_sha={head}&status=failure&per_page={most}"),
                    self.token,
                );
                let body = self.asked(http, asked, RUNS).await?;
                let runs: Runs =
                    herald_wire::read(RUNS, parse, &body).map_err(Unpolled::unreadable)?;
                let mut planned = Vec::new();
                for run in runs.workflow_runs.into_iter().take(most) {
                    let asked = self.origin.get(
                        &format!(
                            "/actions/runs/{}/jobs?filter=latest&per_page={PAGE}",
                            run.id
                        ),
                        self.token,
                    );
                    let body = self.asked(http, asked, JOBS).await?;
                    let jobs: Jobs =
                        herald_wire::read(JOBS, parse, &body).map_err(Unpolled::unreadable)?;
                    let ran = jobs.jobs.iter().any(|job| {
                        job.name == check && job.conclusion.as_deref().is_some_and(failed)
                    });
                    if ran {
                        planned.push(Rerun {
                            run: run.id.to_string(),
                            page: run.html_url,
                            call: format!(
                                "POST {api}/repos/{path}/actions/runs/{}/rerun-failed-jobs",
                                run.id
                            ),
                        });
                    }
                }
                planned
            }
        };
        Ok(match (runs.is_empty(), self.origin.forge()) {
            (false, _) => Plan::Runs(runs),
            (true, Forge::GitLab) => Plan::Unplanned(NO_PIPELINE),
            (true, Forge::GitHub | Forge::Forgejo) => Plan::Unplanned(NO_RUN),
        })
    }
}

#[cfg(test)]
mod tests;
