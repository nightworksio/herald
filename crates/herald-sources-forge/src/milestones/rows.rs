//! A release's rows read from a repository: the open issues carrying each
//! open milestone its selector matches (ADR-0084).

use herald_core::{MOST_ISSUE_ROWS, OpenIssue, Visibility, unreserved};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Http as _};
use serde::Deserialize;

use super::Milestone;
use super::header::Holder;
use crate::Forge;
use crate::limited::{checked, limited};
use crate::queue::capacity::UNSEEN;

/// Why a GitHub repository's issues could not be listed where it refused.
pub const GITHUB_ISSUES_REFUSED: &str = "the token may not list its issues: a GitHub App or fine-grained token needs Issues: read, a classic token repo (public_repo for a public repository)";

/// Why a GitLab project's issues could not be listed where it refused.
pub const GITLAB_ISSUES_REFUSED: &str =
    "the token may not list its issues: it needs read_api and the Guest role or higher";

/// Why a Forgejo repository's issues could not be listed where it refused.
pub const FORGEJO_ISSUES_REFUSED: &str =
    "the token may not list its issues: it needs read access to the repository's issues";

/// The open issues carrying each of `matched`, the milestones of the
/// repository at `origin`, by one request each, at most
/// [`MOST_ISSUE_ROWS`], read with `token` through `http` and seen as
/// `visibility`, or as private where confidential; with each reason, once,
/// that they are not all of them.
///
/// # Errors
///
/// Where a request could not be asked, the forge's rate limit was reached,
/// or it answered what is no listing.
pub(super) async fn rows(
    http: &Erased<'_, dyn DynHttp + '_>,
    holder: Holder<'_>,
    (matched, visibility): (&[Milestone], Visibility),
) -> Result<(Vec<OpenIssue>, Vec<String>), Unpolled> {
    let mut issues = Vec::new();
    let mut reasons: Vec<String> = Vec::new();
    for milestone in matched {
        let (carried, unread) = carried(http, holder, (milestone, visibility)).await?;
        issues.extend(carried);
        if let Some(reason) = unread.filter(|reason| !reasons.contains(reason)) {
            reasons.push(reason);
        }
    }
    if issues.len() > MOST_ISSUE_ROWS {
        reasons.push(format!(
            "{} open issues carry its milestones; only the first {MOST_ISSUE_ROWS} are rows",
            issues.len()
        ));
        issues.truncate(MOST_ISSUE_ROWS);
    }
    Ok((issues, reasons))
}

/// The open issues carrying `milestone` of the repository at `origin`,
/// read with `token` from one page of the source's `page` through `http`,
/// each seen as `visibility`, or as private where it is confidential; with
/// why they are not all of them where the forge refused or the page
/// filled.
///
/// # Errors
///
/// Where a request could not be asked, the forge's rate limit was reached,
/// or it answered what is no listing.
async fn carried(
    http: &Erased<'_, dyn DynHttp + '_>,
    holder: Holder<'_>,
    (milestone, visibility): (&Milestone, Visibility),
) -> Result<(Vec<OpenIssue>, Option<String>), Unpolled> {
    let origin = holder.origin;
    let forge = origin.forge();
    let what = crate::issues::LISTED.on(forge);
    let reading = origin.reading();
    let size = reading.page.max(1);
    let path = match forge {
        Forge::GitHub => format!(
            "/issues?milestone={}&state=open&per_page={size}",
            milestone.key
        ),
        Forge::GitLab => format!(
            "/issues?milestone={}&state=opened&per_page={size}",
            unreserved(&milestone.title)
        ),
        Forge::Forgejo => format!(
            "/issues?state=open&type=issues&milestones={}&limit={size}",
            milestone.key
        ),
    };
    let answer = http
        .send(holder.get(&path)?)
        .await
        .map_err(Unpolled::unreachable)?;
    if let Some(limited) = limited(forge, what, &answer) {
        return Err(limited);
    }
    let refused = match (forge, answer.status) {
        (_, 404) => Some(UNSEEN),
        (Forge::GitHub, 401 | 403) => Some(GITHUB_ISSUES_REFUSED),
        (Forge::GitLab, 401 | 403) => Some(GITLAB_ISSUES_REFUSED),
        (Forge::Forgejo, 401 | 403) => Some(FORGEJO_ISSUES_REFUSED),
        _ => None,
    };
    if let Some(reason) = refused {
        return Ok((Vec::new(), Some(reason.to_owned())));
    }
    checked(forge, what, &answer)?;
    let listed: Vec<Placed> =
        herald_wire::read(what, reading.parse, &answer.body).map_err(Unpolled::unreadable)?;
    let filled = (listed.len() >= size).then(|| {
        format!(
            "its milestone {} listed more than {size} open issues; only those {size} are read",
            milestone.title
        )
    });
    let issues = crate::issues::listed(forge, reading.parse, &answer.body)
        .map_err(Unpolled::unreadable)?
        .into_iter()
        .zip(listed)
        .map(|(issue, placed)| OpenIssue {
            repository: placed.repository(holder),
            visibility: if issue.confidential {
                Visibility::Private
            } else {
                visibility
            },
            number: issue.number,
            title: issue.title,
            url: issue.url,
            milestone: milestone.title.clone(),
        })
        .collect();
    Ok((issues, filled))
}

/// Where an issue of a listing is held, as GitLab names it.
#[derive(Deserialize)]
struct Placed {
    /// Its references, where the forge names them.
    references: Option<References>,
}

/// GitLab's references of an issue.
#[derive(Deserialize)]
struct References {
    /// Its reference from anywhere, its project's full path, `#` and its
    /// number.
    full: String,
}

impl Placed {
    /// The full path of the repository holding it, read from `holder`: a
    /// GitLab group's, the project its reference names, the group where it
    /// names none; any other's, that repository.
    fn repository(self, holder: Holder<'_>) -> String {
        let named = self
            .references
            .and_then(|references| {
                references
                    .full
                    .rsplit_once('#')
                    .map(|(path, _)| path.to_owned())
            })
            .filter(|_| holder.group.is_some());
        named.unwrap_or_else(|| holder.path().to_owned())
    }
}

#[cfg(test)]
mod tests;
