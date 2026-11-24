//! A release's header read from a repository: its open milestones whose
//! titles a release's selector matches, and the open issues they carry
//! where those are its rows (ADR-0082, ADR-0084); or a GitLab group's
//! own (ADR-0086).

use herald_core::{
    Glob, Headed, Header, HeaderRead, MOST_HEADER_MILESTONES, OpenMilestone, Unread,
};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Http as _, Request};
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

/// Where a release's header reads milestones: the repository at `origin`,
/// or the GitLab group at `group` on its API, read with `token`.
#[derive(Clone, Copy)]
pub(super) struct Holder<'a> {
    /// The repository, or the API a group is read on.
    pub(super) origin: &'a Origin,
    /// The token it is read with.
    pub(super) token: Option<&'a Token>,
    /// The full path of the GitLab group whose own milestones are read.
    pub(super) group: Option<&'a str>,
}

impl Holder<'_> {
    /// The request for `path` under it.
    pub(super) fn get(&self, path: &str) -> Result<Request, Unpolled> {
        match self.group {
            Some(group) => gitlab::grouped_request(self.origin, self.token, (group, path)),
            None => self.origin.get(path, self.token),
        }
    }

    /// Its full path.
    pub(super) fn path(&self) -> &str {
        self.group.unwrap_or_else(|| self.origin.path())
    }
}

/// The open milestones of the repository at `origin`, or of the GitLab
/// group `named` names where it names one, whose titles `glob` matches, at most [`MOST_HEADER_MILESTONES`], read with `token` from one
/// page of its listing through `http` and seen as `named` is: where
/// `heading` names them in the header, each overdue where its due day
/// ended before `now`'s day in UTC, and on GitLab its issues counted by
/// one more request each; where `heading` makes their issues the rows,
/// each one's open issues by one more request each, at most
/// [`herald_core::MOST_ISSUE_ROWS`]; the repository named unread with why
/// where the forge would not list them, where a page held more than it
/// read, and where more matched or were carried than it names.
///
/// # Errors
///
/// Where a request could not be asked, the forge's rate limit was reached,
/// or it answered what is no listing.
pub(crate) async fn header(
    http: &Erased<'_, dyn DynHttp + '_>,
    (origin, token): (&Origin, Option<&Token>),
    (named, glob): (&Headed, &Glob),
    (heading, now): (HeaderRead, Timestamp),
) -> Result<Header, Unpolled> {
    let holder = Holder {
        origin,
        token,
        group: named.group.then_some(named.path.as_str()),
    };
    let visibility = named.visibility;
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
        repository: holder.path().to_owned(),
        visibility,
        reason,
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
        (Forge::GitHub, 401 | 403) => Some(GITHUB_REFUSED),
        (Forge::GitLab, 401 | 403) => Some(GITLAB_REFUSED),
        (Forge::Forgejo, 401 | 403) => Some(FORGEJO_REFUSED),
        _ => None,
    };
    let mut header = Header {
        issues: heading.issues.then(Vec::new),
        ..Header::default()
    };
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
    if let Some(issues) = header.issues.as_mut() {
        let (carried, reasons) = super::rows::rows(http, holder, (&matched, visibility)).await?;
        *issues = carried;
        header.unread.extend(reasons.into_iter().map(unread));
    }
    if !heading.header {
        return Ok(header);
    }
    let today = now.to_zoned(TimeZone::UTC).date();
    for milestone in matched {
        let counts = if forge == Forge::GitLab {
            let path = gitlab::statistics_path(&milestone.title);
            let answer = http
                .send(holder.get(&path)?)
                .await
                .map_err(Unpolled::unreachable)?;
            checked(forge, gitlab::STATISTICS, &answer)?;
            Some(gitlab::counted(reading.parse, &answer.body).map_err(Unpolled::unreadable)?)
        } else {
            milestone.counts
        };
        let due = milestone.due_day();
        header.milestones.push(OpenMilestone {
            repository: holder.path().to_owned(),
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
