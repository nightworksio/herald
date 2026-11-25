//! A rule's plan to re-run a flaky check: the failed runs on a pull
//! request's head that ran it, each with its failed job of that check and
//! the exact call that re-runs that job alone (ADR-0065, ADR-0068).

use herald_core::Rerun;
use herald_kinds::{Plan, Unpolled};
use herald_ports::{DynHttp, Erased};
use serde::Deserialize;

use super::{Opened, PAGE, failed};
use crate::{Forge, Origin, Token};

/// What a head's failed GitHub Actions runs are read as.
const RUNS: &str = "a head's failed workflow runs";

/// What a run's jobs are read as.
const JOBS: &str = "a workflow run's jobs";

/// What a head's failed GitLab pipelines are read as.
const PIPELINES: &str = "a head's failed pipelines";

/// What a pipeline's failed jobs are read as.
const FAILED_JOBS: &str = "a pipeline's failed jobs";

/// Why a check no failed GitHub Actions run ran cannot be re-run.
pub const NO_RUN: &str =
    "no failed GitHub Actions run on the head ran this check, so herald has nothing to re-run";

/// Why a GitLab merge request with no failed pipeline cannot be re-run.
pub const NO_PIPELINE: &str =
    "no failed pipeline on the head ran this check, so herald has nothing to re-run";

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
    id: u64,
    name: String,
    conclusion: Option<String>,
}

#[derive(Deserialize)]
struct PipelineJob {
    id: u64,
    name: String,
}

#[derive(Deserialize)]
struct Pipeline {
    id: u64,
    web_url: String,
}

impl Opened<'_> {
    /// The failed runs on `head` that ran the check `check`, the newest
    /// `most` read, each with its failed job of that name and the exact
    /// call re-running that job alone, asked through `http`: GitHub
    /// Actions' runs and GitLab's pipelines whose job of that name failed,
    /// and nothing on Forgejo, which says why.
    ///
    /// # Errors
    ///
    /// Where the runs could not be asked or read.
    pub async fn planned(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        failed: (&str, &str),
        most: usize,
    ) -> Result<Plan, Unpolled> {
        planned((self.origin, self.token), http, failed, most).await
    }
}

/// The failed runs of `origin` on `head` that ran the check `check`, read
/// with `token`, the newest `most` read, each with its failed job of that
/// name and the exact call re-running that job alone, asked through
/// `http`: GitHub Actions' runs and GitLab's pipelines whose job of that
/// name failed, and nothing on Forgejo, which says why.
///
/// # Errors
///
/// Where the runs could not be asked or read.
pub(crate) async fn planned(
    (origin, token): (&Origin, Option<&Token>),
    http: &Erased<'_, dyn DynHttp + '_>,
    (head, check): (&str, &str),
    most: usize,
) -> Result<Plan, Unpolled> {
    let parse = origin.reading().parse;
    let (api, path) = (origin.api(), origin.path());
    let runs = match origin.forge() {
        Forge::Forgejo => return Ok(Plan::Unplanned(UNRERUN)),
        Forge::GitLab => {
            let checked = origin.checked()?;
            let asked = checked.get(
                &format!("/pipelines?sha={head}&status=failed&per_page={most}"),
                token,
            );
            let body = super::asked(origin, http, asked, PIPELINES).await?;
            let pipelines: Vec<Pipeline> =
                herald_wire::read(PIPELINES, parse, &body).map_err(Unpolled::unreadable)?;
            let project = crate::gitlab::project(api, path);
            let mut planned = Vec::new();
            for pipeline in pipelines.into_iter().take(most) {
                let asked = checked.get(
                    &format!(
                        "/pipelines/{}/jobs?scope=failed&per_page={PAGE}",
                        pipeline.id
                    ),
                    token,
                );
                let body = super::asked(origin, http, asked, FAILED_JOBS).await?;
                let jobs: Vec<PipelineJob> =
                    herald_wire::read(FAILED_JOBS, parse, &body).map_err(Unpolled::unreadable)?;
                if let Some(job) = jobs.iter().find(|job| job.name == check) {
                    planned.push(Rerun {
                        run: pipeline.id.to_string(),
                        job: job.id.to_string(),
                        page: pipeline.web_url,
                        call: format!("POST {project}/jobs/{}/retry", job.id),
                    });
                }
            }
            planned
        }
        Forge::GitHub => {
            let checked = origin.checked()?;
            let asked = checked.get(
                &format!("/actions/runs?head_sha={head}&status=failure&per_page={most}"),
                token,
            );
            let body = super::asked(origin, http, asked, RUNS).await?;
            let runs: Runs = herald_wire::read(RUNS, parse, &body).map_err(Unpolled::unreadable)?;
            let mut planned = Vec::new();
            for run in runs.workflow_runs.into_iter().take(most) {
                let asked = checked.get(
                    &format!(
                        "/actions/runs/{}/jobs?filter=latest&per_page={PAGE}",
                        run.id
                    ),
                    token,
                );
                let body = super::asked(origin, http, asked, JOBS).await?;
                let jobs: Jobs =
                    herald_wire::read(JOBS, parse, &body).map_err(Unpolled::unreadable)?;
                let ran = jobs
                    .jobs
                    .iter()
                    .find(|job| job.name == check && job.conclusion.as_deref().is_some_and(failed));
                if let Some(job) = ran {
                    planned.push(Rerun {
                        run: run.id.to_string(),
                        job: job.id.to_string(),
                        page: run.html_url,
                        call: format!("POST {api}/repos/{path}/actions/jobs/{}/rerun", job.id),
                    });
                }
            }
            planned
        }
    };
    Ok(match (runs.is_empty(), origin.forge()) {
        (false, _) => Plan::Runs(runs),
        (true, Forge::GitLab) => Plan::Unplanned(NO_PIPELINE),
        (true, Forge::GitHub | Forge::Forgejo) => Plan::Unplanned(NO_RUN),
    })
}

#[cfg(test)]
mod tests;
