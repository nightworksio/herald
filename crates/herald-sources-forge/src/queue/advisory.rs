//! A view's advisories overview: each repository's open security alerts,
//! read from its own listings or its owner's, and each listing that could
//! not be read named with the permission it needs (ADR-0067).

use herald_core::{Advisories, Advisory, Unread, Visibility};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Http as _, Method, Request, Response, SecretText};
use herald_wire::answered;

use crate::alerts::{ForgeAlert, OPENED, Scanner, github_listed, opened, opened_listing};
use crate::forge::{OWNER, authorized, whole};
use crate::limited::limited;
use crate::options::{AlertAction, Tool};
use crate::{Forge, Origin, Token};

/// The most alerts one page of a listing asks for.
const ADVISORY_PAGE: usize = 100;

/// What GitHub's secret scanning listing is asked with so it answers no
/// secret's value (ADR-0067).
pub(crate) const HIDE_SECRET: &str = "&hide_secret=true";

/// Why a GitHub repository's alerts of one scanner could not be read where
/// it refused.
pub const GITHUB_REFUSED: &str = "the token may not read them, or the scanner is off for the repository: a GitHub App or fine-grained token needs Dependabot alerts, Code scanning alerts and Secret scanning alerts: read; a classic token security_events, and repo for a private repository";

/// Why a GitHub organization's alerts of one scanner could not be read
/// where it refused.
pub const GITHUB_ORGANIZATION_REFUSED: &str = "the token may not read the organization's alerts: it must act for an organization owner or security manager, a GitHub App or fine-grained token with Dependabot alerts, Code scanning alerts and Secret scanning alerts: read; a classic token with security_events and read:org";

/// Why a GitHub listing of alerts could not be read where the forge found
/// none.
pub const GITHUB_UNSEEN: &str = "the forge found no such listing: the token may not see the repository or organization, or the scanner is not set up for it";

/// Why a GitLab project's or group's vulnerabilities could not be read.
pub const GITLAB_REFUSED: &str = "the token may not read its vulnerabilities: it needs read_api and the Developer role or higher, on GitLab Ultimate";

/// Why a Forgejo repository's or owner's advisories are unread.
pub const FORGEJO_UNRAISED: &str = "Forgejo raises no security alerts";

/// Whose listing of alerts is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Holder<'a> {
    /// A repository's, at its full path.
    Repository(&'a str),
    /// An organization's or group's, by its name or path.
    Owner(&'a str),
}

impl Holder<'_> {
    /// The path it is named by.
    const fn path(&self) -> &str {
        match self {
            Self::Repository(path) | Self::Owner(path) => path,
        }
    }
}

/// What reads a listing of alerts: the forge's API, how much one answer
/// parses, and the token.
pub(crate) struct Reader<'a> {
    pub(crate) origin: &'a Origin,
    pub(crate) token: Option<&'a Token>,
}

/// What finds the member an owner's listing names by its full path.
type Member<'m> = &'m (dyn Fn(&str) -> Option<String> + Sync);

/// Where a listing stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cut {
    /// At its end: every open alert of it was read.
    Whole,
    /// Past the most wanted: an advisory of a repository read was left out.
    Held,
    /// At its last page read, `pages`, with more pages it did not read.
    Short {
        /// The pages read.
        pages: usize,
    },
}

/// One listing's advisories, and where it stopped.
type Paged = (Vec<Advisory>, Cut);

/// The URL of the next page `answer` names, where it names one.
fn next_page(answer: &Response) -> Option<String> {
    let link = answer.header("link")?;
    link.split(',').find_map(|part| {
        let (url, params) = part.split_once(';')?;
        params
            .split(';')
            .any(|param| param.trim() == "rel=\"next\"")
            .then(|| {
                url.trim()
                    .trim_start_matches('<')
                    .trim_end_matches('>')
                    .to_owned()
            })
    })
}

/// Where a listing of `holder` that kept `kept` advisories, at most `left`
/// wanted, stops before its end, after its page `page` of at most `pages`,
/// a next page to be read where `next` says: none while it reads on or
/// where it ends, which its caller tells by the next page it lacks. Only
/// advisories of a repository it keeps count, so another repository's
/// alerts never cut one short; an owner's listing past its pages stops
/// short, never saying an advisory was left out where it cannot know.
const fn stops(
    holder: Holder<'_>,
    (kept, left): (usize, usize),
    (page, pages): (usize, usize),
    next: bool,
) -> Option<Cut> {
    let whole = matches!(holder, Holder::Repository(_));
    if kept > left || (whole && next && kept == left) {
        Some(Cut::Held)
    } else if next && page >= pages {
        Some(Cut::Short { pages: page })
    } else {
        None
    }
}

/// `alert` as a section shows it, of the repository at `repository`: a
/// secret's type alone, never its severity, path or what it found.
fn advisory(alert: ForgeAlert, repository: &str) -> Advisory {
    let secret = alert.tool == Tool::SecretScanning;
    Advisory {
        repository: repository.to_owned(),
        tool: alert.tool.name().to_owned(),
        severity: alert
            .severity
            .filter(|_| !secret)
            .map(|severity| severity.name().to_owned()),
        summary: alert.summary,
        state: match alert.action {
            AlertAction::Confirmed => "confirmed",
            _ => "open",
        }
        .to_owned(),
        url: alert.url,
        opened: alert.occurred,
    }
}

impl Reader<'_> {
    /// The GitHub request for the first page of `scanner`'s open alerts of
    /// `holder`.
    fn github_first(&self, scanner: Scanner, holder: Holder<'_>) -> Result<Request, Unpolled> {
        let hidden = if scanner == Scanner::SecretScanning {
            HIDE_SECRET
        } else {
            ""
        };
        let path = format!(
            "/{}/alerts?state=open&per_page={ADVISORY_PAGE}{hidden}",
            scanner.segment()
        );
        match holder {
            Holder::Repository(_) => self.origin.get(&path, self.token),
            Holder::Owner(owner) => Ok(self
                .origin
                .rooted(&format!("/orgs/{}{path}", whole(owner, OWNER)?), self.token)),
        }
    }

    /// `scanner`'s open advisories of `holder` that `kept` names a
    /// repository of, at most `left`, each page after the first asked
    /// where the last names it under the API; or why the forge would not
    /// list them.
    async fn github(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (scanner, holder, kept): (Scanner, Holder<'_>, Kept<'_>),
        left: usize,
    ) -> Result<Result<Paged, String>, Unpolled> {
        let parse = self.origin.reading().parse;
        let refused = match holder {
            Holder::Repository(_) => GITHUB_REFUSED,
            Holder::Owner(_) => GITHUB_ORGANIZATION_REFUSED,
        };
        let mut request = self.github_first(scanner, holder)?;
        let mut alerts = Vec::new();
        let pages = left.div_ceil(ADVISORY_PAGE);
        let mut page = 0;
        loop {
            page += 1;
            let answer = http.send(request).await.map_err(Unpolled::unreachable)?;
            if let Some(limited) = limited(Forge::GitHub, scanner.listed_what(), &answer) {
                return Err(limited);
            }
            let reason = match answer.status {
                401 | 403 => Some(refused),
                404 => Some(GITHUB_UNSEEN),
                _ => None,
            };
            if let Some(reason) = reason {
                return Ok(Err(format!("{}: {reason}", scanner.listed_what())));
            }
            answered(scanner.listed_what(), &answer).map_err(Unpolled::unreadable)?;
            let found =
                github_listed(scanner, parse, &answer.body).map_err(Unpolled::unreadable)?;
            alerts.extend(
                found
                    .into_iter()
                    .filter(|alert| alert.action == AlertAction::Created)
                    .filter_map(kept),
            );
            let next = next_page(&answer);
            let under = format!("{}/", self.origin.api());
            let elsewhere = next.as_ref().is_some_and(|next| !next.starts_with(&under));
            let last = if elsewhere { page } else { pages };
            if let Some(cut) = stops(holder, (alerts.len(), left), (page, last), next.is_some()) {
                alerts.truncate(left);
                return Ok(Ok((alerts, cut)));
            }
            let Some(next) = next else {
                return Ok(Ok((alerts, Cut::Whole)));
            };
            let asked = Request::new(Method::Get, SecretText::plain(next)).at_most(parse);
            request = authorized(Forge::GitHub, asked, self.token);
        }
    }

    /// GitLab's open vulnerabilities of `holder` that `kept` names a
    /// repository of, at most `left`; or why the forge would not list them.
    async fn gitlab(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (holder, kept): (Holder<'_>, Kept<'_>),
        left: usize,
    ) -> Result<Result<Paged, String>, Unpolled> {
        let parse = self.origin.reading().parse;
        let endpoint = crate::gitlab::graphql(self.origin.api());
        let group = matches!(holder, Holder::Owner(_));
        let mut after: Option<String> = None;
        let mut alerts = Vec::new();
        let pages = left.div_ceil(ADVISORY_PAGE);
        let mut page = 0;
        loop {
            page += 1;
            let request = opened_listing(
                &endpoint,
                (group, holder.path(), self.token),
                (after.as_deref(), ADVISORY_PAGE, parse),
            );
            let answer = http.send(request).await.map_err(Unpolled::unreachable)?;
            if let Some(limited) = limited(Forge::GitLab, OPENED, &answer) {
                return Err(limited);
            }
            if matches!(answer.status, 401 | 403) {
                return Ok(Err(GITLAB_REFUSED.to_owned()));
            }
            answered(OPENED, &answer).map_err(Unpolled::unreadable)?;
            let Some((found, next)) = opened(parse, &answer.body).map_err(Unpolled::unreadable)?
            else {
                return Ok(Err(GITLAB_REFUSED.to_owned()));
            };
            alerts.extend(found.into_iter().filter_map(kept));
            if let Some(cut) = stops(holder, (alerts.len(), left), (page, pages), next.is_some()) {
                alerts.truncate(left);
                return Ok(Ok((alerts, cut)));
            }
            let Some(next) = next else {
                return Ok(Ok((alerts, Cut::Whole)));
            };
            after = Some(next);
        }
    }

    /// The open advisories of `holder`, at most `most`, at `at`: on an
    /// owner, only those of a repository `member` finds, named by its
    /// path; each listing that could not be read named in `unread`.
    pub(crate) async fn read(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (holder, member): (Holder<'_>, Member<'_>),
        (most, at): (usize, jiff::Timestamp),
    ) -> Result<Advisories, Unpolled> {
        let mut found = Advisories::none(at);
        let unread = |reason: String| Unread {
            repository: holder.path().to_owned(),
            visibility: Visibility::Private,
            reason,
        };
        let listings: Vec<Option<Scanner>> = match self.origin.forge() {
            Forge::GitHub => Scanner::ALL.into_iter().map(Some).collect(),
            Forge::GitLab => vec![None],
            Forge::Forgejo => {
                found.unread.push(unread(FORGEJO_UNRAISED.to_owned()));
                return Ok(found);
            }
        };
        let kept = |alert: ForgeAlert| {
            let repository = match holder {
                Holder::Repository(path) => Some(path.to_owned()),
                Holder::Owner(_) => member(alert.repository.as_deref()?),
            }?;
            Some(advisory(alert, &repository))
        };
        for scanner in listings {
            let left = most.saturating_sub(found.open.len());
            if left == 0 {
                found.more = true;
                break;
            }
            let read = match scanner {
                Some(scanner) => self.github(http, (scanner, holder, &kept), left).await?,
                None => self.gitlab(http, (holder, &kept), left).await?,
            };
            let what = scanner.map_or(OPENED, Scanner::listed_what);
            match read {
                Ok((advisories, cut)) => {
                    found.open.extend(advisories);
                    match (cut, holder) {
                        (Cut::Whole, _) => {}
                        (Cut::Held, _) | (Cut::Short { .. }, Holder::Repository(_)) => {
                            found.more = true;
                        }
                        (Cut::Short { pages }, Holder::Owner(_)) => {
                            found.unread.push(unread(short(what, pages)));
                        }
                    }
                }
                Err(reason) => found.unread.push(unread(reason)),
            }
        }
        Ok(found)
    }
}

/// Why the rest of an owner's listing `what` was not read after `pages`
/// pages: it may hold advisories of the repositories the view holds.
fn short(what: &str, pages: usize) -> String {
    format!(
        "{what}: only its first {pages} pages were read, as most_advisories bounds the cost, so later pages may hold more of the repositories the view holds"
    )
}

/// What keeps an alert of a listing as an advisory: of the repository a
/// repository's listing is, or of a member an owner's names.
type Kept<'k> = &'k (dyn Fn(ForgeAlert) -> Option<Advisory> + Sync);

#[cfg(test)]
mod tests;
