//! A release's header read from a repository: its open milestones whose
//! titles a release's selector matches (ADR-0082).

use herald_core::{Glob, Header, MOST_HEADER_MILESTONES, OpenMilestone, Unread, Visibility};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Http as _};
use jiff::Timestamp;
use jiff::tz::TimeZone;

use super::{LISTED, Milestone, gitlab, read_listed};
use crate::limited::{checked, limited};
use crate::queue::capacity::UNSEEN;
use crate::{Forge, Origin, Token};

/// Why a GitHub repository's milestones could not be read where it
/// refused.
pub const GITHUB_REFUSED: &str = "the token may not read its milestones: a GitHub App or fine-grained token needs Issues: read or Pull requests: read, a classic token repo (public_repo for a public repository)";

/// Why a GitLab project's milestones could not be read where it refused.
pub const GITLAB_REFUSED: &str = "the token may not read its milestones or issue statistics: it needs read_api and the Guest role or higher";

/// Why a Forgejo repository's milestones could not be read where it
/// refused.
pub const FORGEJO_REFUSED: &str =
    "the token may not read its milestones: it needs read access to the repository's issues";

/// The open milestones of the repository at `origin` whose titles `glob`
/// matches, at most [`MOST_HEADER_MILESTONES`], read with `token` from one
/// page of its listing through `http` and seen as `visibility`, each
/// overdue where its due day ended before `now`'s day in UTC, and on
/// GitLab its issues counted by one more request each; the repository
/// named unread with why where the forge would not list them, where the
/// page held more than it read, and where more matched than it names.
///
/// # Errors
///
/// Where a request could not be asked, the forge's rate limit was reached,
/// or it answered what is no listing.
pub(crate) async fn header(
    http: &Erased<'_, dyn DynHttp + '_>,
    (origin, token): (&Origin, Option<&Token>),
    (glob, visibility): (&Glob, Visibility),
    now: Timestamp,
) -> Result<Header, Unpolled> {
    let forge = origin.forge();
    let what = LISTED.on(forge);
    let reading = origin.reading();
    let size = reading.page.max(1);
    let path = match forge {
        Forge::GitHub => format!("/milestones?state=open&per_page={size}"),
        Forge::GitLab => format!("/milestones?state=active&per_page={size}"),
        Forge::Forgejo => format!("/milestones?state=open&limit={size}"),
    };
    let unread = |reason: String| Unread {
        repository: origin.path().to_owned(),
        visibility,
        reason,
    };
    let answer = http
        .send(origin.get(&path, token)?)
        .await
        .map_err(Unpolled::unreachable)?;
    if let Some(limited) = limited(forge, what, &answer) {
        return Err(limited);
    }
    let refused = match (forge, answer.status) {
        (_, 404) => Some(UNSEEN),
        (Forge::GitHub, 401 | 403) => Some(GITHUB_REFUSED),
        (Forge::GitLab, 401 | 403) => Some(GITLAB_REFUSED),
        (Forge::Forgejo, 401 | 403) => Some(FORGEJO_REFUSED),
        _ => None,
    };
    let mut header = Header::default();
    if let Some(reason) = refused {
        header.unread.push(unread(reason.to_owned()));
        return Ok(header);
    }
    checked(forge, what, &answer)?;
    let listed =
        read_listed(forge, what, reading.parse, &answer.body).map_err(Unpolled::unreadable)?;
    if listed.len() >= size {
        header.unread.push(unread(format!(
            "its listing held more than {size} open milestones; only those {size} are matched"
        )));
    }
    let mut matched: Vec<Milestone> = listed
        .into_iter()
        .filter(|milestone| glob.matches(&milestone.title))
        .collect();
    if matched.len() > MOST_HEADER_MILESTONES {
        header.unread.push(unread(format!(
            "{} open milestones match; only the first {MOST_HEADER_MILESTONES} are named",
            matched.len()
        )));
        matched.truncate(MOST_HEADER_MILESTONES);
    }
    let today = now.to_zoned(TimeZone::UTC).date();
    for milestone in matched {
        let counts = if forge == Forge::GitLab {
            let path = gitlab::statistics_path(&milestone.title);
            let answer = http
                .send(origin.get(&path, token)?)
                .await
                .map_err(Unpolled::unreachable)?;
            checked(forge, gitlab::STATISTICS, &answer)?;
            Some(gitlab::counted(reading.parse, &answer.body).map_err(Unpolled::unreadable)?)
        } else {
            milestone.counts
        };
        let due = milestone.due_day();
        header.milestones.push(OpenMilestone {
            repository: origin.path().to_owned(),
            visibility,
            overdue: due.is_some_and(|due| due < today),
            due,
            title: milestone.title,
            url: milestone.url,
            counts,
        });
    }
    Ok(header)
}

#[cfg(test)]
mod tests;
