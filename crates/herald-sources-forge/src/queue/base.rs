//! A release's base branch read: its head's combined check state, each
//! request asked on the validators kept from the last read (ADR-0083).

use herald_core::{BaseRead, Checks, KeptBase, KeptCheck, joined_checks, unreserved};
use herald_kinds::{BaseFound, Unpolled};
use herald_ports::{DynHttp, Erased, Http as _};
use serde::Deserialize;

use super::detail::{self, CHECKS};
use super::gitlab::PIPELINE;
use super::{PAGE, failed};
use crate::forge::validators;
use crate::limited::{checked, limited};
use crate::queue::capacity::UNSEEN;
use crate::{Forge, Origin, Token};

/// What a base branch's head's checks are read as.
const BASE: &str = "a base branch's checks";

/// What a GitLab base branch's failed jobs are read as.
const FAILED_JOBS: &str = "a base branch's failed jobs";

/// Why a GitHub base branch's checks could not be read where it refused.
pub const GITHUB_REFUSED: &str = "the token may not read its base branch's checks: a GitHub App or fine-grained token needs Checks: read and Commit statuses: read, a classic token repo (public_repo for a public repository)";

/// Why a GitLab base branch's pipeline could not be read where it refused.
pub const GITLAB_REFUSED: &str = "the token may not read its base branch's pipeline: it needs read_api and the Reporter role or higher";

/// Why a Forgejo base branch's statuses could not be read where it
/// refused.
pub const FORGEJO_REFUSED: &str = "the token may not read its base branch's statuses: it needs read access to the repository's code";

/// The GitLab pipeline states that end in failure.
pub(crate) const PIPELINE_FAILED: [&str; 2] = ["failed", "canceled"];

/// The GitLab pipeline states that end with nothing failed.
pub(crate) const PIPELINE_PASSED: [&str; 2] = ["success", "skipped"];

#[derive(Deserialize)]
struct Pipeline {
    id: u64,
    status: String,
}

#[derive(Deserialize)]
struct Job {
    name: String,
    #[serde(default)]
    allow_failure: bool,
}

#[derive(Deserialize)]
struct Combined {
    #[serde(default)]
    statuses: Vec<Status>,
}

#[derive(Deserialize)]
struct Status {
    context: String,
    state: String,
}

/// The paths, under the repository, of the requests reading `branch`'s
/// head's checks on `forge`, in the order they are asked.
fn paths(forge: Forge, branch: &str) -> Vec<String> {
    let branch = unreserved(branch);
    let page = forge.api().page;
    match forge {
        Forge::GitHub => vec![
            format!("/commits/{branch}/check-runs?{page}={PAGE}"),
            format!("/commits/{branch}/status?{page}={PAGE}"),
        ],
        Forge::GitLab => vec![format!("/pipelines?ref={branch}&{page}=1")],
        Forge::Forgejo => vec![format!("/commits/{branch}/status?{page}={PAGE}")],
    }
}

/// Why `forge` refused a read answered with `status`, where it did.
fn refused(forge: Forge, status: u16) -> Option<&'static str> {
    match (forge, status) {
        (_, 404) => Some(UNSEEN),
        (Forge::GitHub, 401 | 403) => Some(GITHUB_REFUSED),
        (Forge::GitLab, 401 | 403) => Some(GITLAB_REFUSED),
        (Forge::Forgejo, 401 | 403) => Some(FORGEJO_REFUSED),
        _ => None,
    }
}

/// What GitHub's combined status `body` says, parsing no more than
/// `parse` bytes.
fn statuses(body: &[u8], parse: usize) -> Result<Checks, herald_wire::SourceError> {
    let combined: Combined = herald_wire::read(CHECKS, parse, body)?;
    Ok(detail::fold(combined.statuses.into_iter().map(|status| {
        let ran = status.state != detail::RUNNING;
        let failed = failed(&status.state);
        (status.context, ran, failed)
    })))
}

/// What the `index`th request's answer `body` says on `forge`, parsing no
/// more than `parse` bytes; on GitLab, the newest pipeline's state, its
/// failed jobs read by one more request where it failed.
async fn said(
    http: &Erased<'_, dyn DynHttp + '_>,
    (origin, token): (&Origin, Option<&Token>),
    (index, body): (usize, &[u8]),
) -> Result<Checks, Unpolled> {
    let forge = origin.forge();
    let parse = origin.reading().parse;
    match (forge, index) {
        (Forge::GitHub, 1) => statuses(body, parse).map_err(Unpolled::unreadable),
        (Forge::GitHub | Forge::Forgejo, _) => {
            detail::checks(forge, body, parse).map_err(Unpolled::unreadable)
        }
        (Forge::GitLab, _) => {
            let listed: Vec<Pipeline> =
                herald_wire::read(BASE, parse, body).map_err(Unpolled::unreadable)?;
            let Some(pipeline) = listed.into_iter().next() else {
                return Ok(Checks::Absent);
            };
            let status = pipeline.status.as_str();
            if PIPELINE_PASSED.contains(&status) {
                return Ok(Checks::Passed);
            }
            if !PIPELINE_FAILED.contains(&status) {
                return Ok(Checks::Pending);
            }
            let page = forge.api().page;
            let path = format!(
                "/pipelines/{}/jobs?scope[]=failed&{page}={PAGE}",
                pipeline.id
            );
            let answer = http
                .send(origin.get(&path, token)?)
                .await
                .map_err(Unpolled::unreachable)?;
            checked(forge, FAILED_JOBS, &answer)?;
            let jobs: Vec<Job> = herald_wire::read(FAILED_JOBS, parse, &answer.body)
                .map_err(Unpolled::unreadable)?;
            let mut names: Vec<String> = jobs
                .into_iter()
                .filter(|job| !job.allow_failure)
                .map(|job| job.name)
                .collect();
            if names.is_empty() {
                names.push(PIPELINE.to_owned());
            }
            Ok(Checks::Failed(names))
        }
    }
}

/// The combined check state of `branch`'s head in the repository at
/// `origin`, read with `token` through `http`, each request conditional on
/// `kept`'s validators where it fits, an unchanged answer read as kept;
/// the branch named unread with why where the forge refused.
///
/// # Errors
///
/// Where a request could not be asked, the forge's rate limit was reached,
/// or it answered what is no check state.
pub(crate) async fn based(
    http: &Erased<'_, dyn DynHttp + '_>,
    (origin, token): (&Origin, Option<&Token>),
    branch: &str,
    kept: Option<&KeptBase>,
) -> Result<BaseFound, Unpolled> {
    let forge = origin.forge();
    let paths = paths(forge, branch);
    let kept = kept.and_then(|kept| kept.fitting(paths.len()));
    let mut parts = Vec::with_capacity(paths.len());
    for (index, path) in paths.iter().enumerate() {
        let before = kept.and_then(|kept| kept.get(index));
        let asked = origin.get(path, token)?.conditional(
            before.and_then(|part| part.validators.etag.as_deref()),
            before.and_then(|part| part.validators.last_modified.as_deref()),
        );
        let answer = http.send(asked).await.map_err(Unpolled::unreachable)?;
        if let Some(limited) = limited(forge, BASE, &answer) {
            return Err(limited);
        }
        if let Some(reason) = refused(forge, answer.status) {
            return Ok(BaseFound {
                read: BaseRead::Unread(reason.to_owned()),
                parts: Vec::new(),
            });
        }
        if let Some(part) = before.filter(|_| Forge::unchanged(&answer)) {
            parts.push(part.clone());
            continue;
        }
        checked(forge, BASE, &answer)?;
        let checks = said(http, (origin, token), (index, &answer.body)).await?;
        parts.push(KeptCheck {
            validators: validators(&answer),
            checks,
        });
    }
    Ok(BaseFound {
        read: BaseRead::Checked(joined_checks(parts.iter().map(|part| part.checks.clone()))),
        parts,
    })
}

#[cfg(test)]
mod tests;
