//! A repository's open pull requests as a view reads them: whose turn each
//! is, from its reviews, checks and mergeability.

use herald_core::{Pull, Visibility};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Http as _, Request};
use herald_wire::answered;

use crate::forge::authorized;
use crate::{Forge, Origin, Token};

mod detail;
mod github;
mod gitlab;
mod listing;

/// The most open pull requests one page lists, on every forge.
pub const PAGE: usize = 100;

pub use herald_kinds::{MOST_OPEN, MOST_PULLS};

/// The requests each pull request read in full takes: its reviews and its
/// head's checks.
pub const PER_PULL: usize = 2;

/// What a failed check or status ends in, on every forge, ignoring case.
pub(crate) const FAILED: [&str; 6] = [
    "FAILURE",
    "TIMED_OUT",
    "CANCELLED",
    "ACTION_REQUIRED",
    "STARTUP_FAILURE",
    "ERROR",
];

/// How a repository's open pull requests are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadPath {
    /// GitHub's GraphQL, a page at a time, everything a view needs in each.
    Graphql,
    /// GitLab's listing, everything a view needs in each page.
    Listing,
    /// A REST listing, then each pull request's reviews and checks.
    Detailed,
}

impl ReadPath {
    /// The path a repository on `forge` is read by, `tokened` where its
    /// source reads with a token: GitHub's GraphQL needs one.
    #[must_use]
    pub const fn of(forge: Forge, tokened: bool) -> Self {
        match (forge, tokened) {
            (Forge::GitHub, true) => Self::Graphql,
            (Forge::GitLab, _) => Self::Listing,
            (Forge::GitHub | Forge::Forgejo, false) | (Forge::Forgejo, true) => Self::Detailed,
        }
    }

    /// Its name, as doctor shows it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Graphql => "graphql",
            Self::Listing => "listing",
            Self::Detailed => "rest",
        }
    }

    /// The most requests a round of it takes for a repository listing at
    /// most `most_open` and reading at most `most_pulls` in full.
    #[must_use]
    pub const fn most_requests(self, most_open: usize, most_pulls: usize) -> usize {
        let pages = most_open.div_ceil(PAGE);
        match self {
            Self::Graphql | Self::Listing => pages,
            Self::Detailed => {
                let read = if most_pulls < most_open {
                    most_pulls
                } else {
                    most_open
                };
                pages + read * PER_PULL
            }
        }
    }
}

/// How a repository's open pull requests are listed.
#[derive(Debug, Clone, Copy)]
pub struct Opened<'a> {
    /// The repository.
    pub origin: &'a Origin,
    /// What it reads with.
    pub token: Option<&'a Token>,
    /// The project its pull requests are shown as.
    pub project: &'a str,
    /// The GraphQL endpoint, on GitHub.
    pub graphql: &'a str,
    /// Who may see it, where its source knows; else the listing says, or it
    /// is taken as private.
    pub visibility: Option<Visibility>,
    /// The most it lists.
    pub most_open: usize,
    /// The most it reads in full, where each takes requests of its own;
    /// those past it read as unknown.
    pub most_pulls: usize,
}

/// What a page names its pull requests by: the project they are shown as,
/// the repository's full path, and who may see it where that is known.
pub(crate) type Named<'a> = (&'a str, &'a str, Option<Visibility>);

/// A page of open pull requests, and where the next begins.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Page {
    pub(crate) pulls: Vec<Pull>,
    pub(crate) next: Option<String>,
}

impl Opened<'_> {
    /// The path it reads by.
    #[must_use]
    pub const fn read_path(&self) -> ReadPath {
        ReadPath::of(self.origin.forge(), self.token.is_some())
    }

    /// The request for the page after `after`.
    fn request(&self, after: Option<&str>, number: usize) -> Request {
        let parse = self.origin.reading().parse;
        let forge = self.origin.forge();
        match self.read_path() {
            ReadPath::Graphql => authorized(
                forge,
                github::request(self.graphql, self.origin.path(), after, parse),
                self.token,
            ),
            ReadPath::Listing => self.origin.get(&gitlab::path(number), self.token),
            ReadPath::Detailed => self.origin.get(&listing::path(forge, number), self.token),
        }
    }

    /// The page `body` holds.
    fn read(&self, body: &[u8]) -> Result<Page, herald_wire::SourceError> {
        let parse = self.origin.reading().parse;
        let named = (self.project, self.origin.path(), self.visibility);
        match self.read_path() {
            ReadPath::Graphql => github::page(body, parse, named),
            ReadPath::Listing => gitlab::page(body, parse, named),
            ReadPath::Detailed => listing::page(self.what(), body, parse, named),
        }
    }

    /// The body of the answer to `request`, read as `what`.
    async fn asked(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        request: Request,
        what: &'static str,
    ) -> Result<Vec<u8>, Unpolled> {
        let answer = http.send(request).await.map_err(Unpolled::unreachable)?;
        answered(what, &answer).map_err(Unpolled::unreadable)?;
        Ok(answer.body)
    }

    /// `pull` with its reviews and its head's checks read.
    async fn detailed(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        pull: &mut Pull,
    ) -> Result<(), Unpolled> {
        let forge = self.origin.forge();
        let parse = self.origin.reading().parse;
        let asked = self
            .origin
            .get(&detail::reviews_path(forge, pull.number), self.token);
        let body = self.asked(http, asked, detail::REVIEWS).await?;
        let requested = pull.review == herald_core::Review::Requested;
        pull.review =
            detail::reviews(forge, &body, parse, requested).map_err(Unpolled::unreadable)?;
        if let Some(sha) = pull.head.as_deref() {
            let asked = self
                .origin
                .get(&detail::checks_path(forge, sha), self.token);
            let body = self.asked(http, asked, detail::CHECKS).await?;
            pull.checks = detail::checks(forge, &body, parse).map_err(Unpolled::unreadable)?;
        }
        Ok(())
    }

    /// Its open pull requests, at most `most`, asked through `http` a page
    /// at a time.
    ///
    /// # Errors
    ///
    /// Where a page could not be asked or read.
    pub async fn pulls(&self, http: &Erased<'_, dyn DynHttp + '_>) -> Result<Vec<Pull>, Unpolled> {
        let what = self.what();
        let mut pulls = Vec::new();
        let mut after: Option<String> = None;
        for number in 1..=self.most_open.div_ceil(PAGE) {
            let asked = self.request(after.as_deref(), number);
            let body = self.asked(http, asked, what).await?;
            let page = self.read(&body).map_err(Unpolled::unreadable)?;
            let more = match self.origin.forge() {
                Forge::GitHub if self.read_path() == ReadPath::Graphql => page.next.is_some(),
                Forge::GitHub | Forge::GitLab => page.pulls.len() >= PAGE,
                Forge::Forgejo => !page.pulls.is_empty(),
            };
            pulls.extend(page.pulls);
            after = page.next;
            if !more {
                break;
            }
        }
        pulls.truncate(self.most_open);
        if self.read_path() == ReadPath::Detailed {
            for (read, pull) in pulls.iter_mut().enumerate() {
                if read < self.most_pulls {
                    self.detailed(http, pull).await?;
                } else {
                    pull.unread = true;
                }
            }
        }
        Ok(pulls)
    }

    /// What its listing is read as.
    const fn what(&self) -> &'static str {
        match self.origin.forge() {
            Forge::GitHub if self.token.is_some() => github::WHAT,
            Forge::GitHub => listing::GITHUB,
            Forge::GitLab => gitlab::WHAT,
            Forge::Forgejo => listing::FORGEJO,
        }
    }
}

/// A source's one repository, its open pull requests read for a view.
#[derive(Debug, Clone)]
pub struct Queued {
    /// The repository.
    pub origin: Origin,
    /// What it reads with.
    pub token: Option<Token>,
    /// The project its pull requests are shown as.
    pub project: String,
    /// The GraphQL endpoint, on GitHub.
    pub graphql: String,
}

impl herald_kinds::Queue for Queued {
    fn opened<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        most: herald_kinds::Most,
    ) -> herald_ports::Boxed<'a, Result<Vec<Pull>, Unpolled>> {
        Box::pin(async move {
            Opened {
                origin: &self.origin,
                token: self.token.as_ref(),
                project: &self.project,
                graphql: &self.graphql,
                visibility: None,
                most_open: most.open,
                most_pulls: most.pulls,
            }
            .pulls(&Erased(http))
            .await
        })
    }
}

/// What reading at most `most` open pull requests of a repository on
/// `forge` costs each round, `tokened` where its source reads with a token.
#[must_use]
pub const fn cost(
    forge: Forge,
    tokened: bool,
    most: herald_kinds::Most,
) -> herald_kinds::QueueCost {
    let path = ReadPath::of(forge, tokened);
    herald_kinds::QueueCost {
        path: path.name(),
        requests: path.most_requests(most.open, most.pulls),
        mergeability: !matches!((forge, path), (Forge::GitHub, ReadPath::Detailed)),
    }
}

#[cfg(test)]
mod tests;
