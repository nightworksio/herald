//! A repository's open pull requests as a view reads them: whose turn each
//! is, from its reviews, checks and mergeability.

use herald_core::{Pull, Visibility};
use herald_kinds::{Asked, Unpolled};
use herald_ports::{DynHttp, Erased, Http as _, Request};
use herald_wire::answered;

use crate::forge::authorized;
use crate::{Forge, Origin, Token};

mod detail;
mod github;
mod gitlab;
mod listing;
mod reconcile;
mod rerun;
mod scope;

pub(crate) use scope::{Scoped, scope_cost};

pub(crate) use gitlab::{PIPELINE, forge_turn};

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

/// The associations GitHub names a reviewer by whose review decides, as
/// its review decision counts them.
pub(crate) const TRUSTED: [&str; 3] = ["OWNER", "MEMBER", "COLLABORATOR"];

/// Whether `state` is one a failed check ends in, ignoring case.
pub(crate) fn failed(state: &str) -> bool {
    FAILED.iter().any(|named| named.eq_ignore_ascii_case(state))
}

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
    pub fn most_requests(self, most_open: usize, most_pulls: usize) -> usize {
        let pages = most_open.div_ceil(PAGE);
        match self {
            Self::Graphql | Self::Listing => pages,
            Self::Detailed => pages + most_pulls.min(most_open) * PER_PULL,
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
    /// Whether a REST listing read without a token takes who may see the
    /// repository from what the listing states, where its source does not
    /// know, rather than taking each row as private (ADR-0062).
    pub stated: bool,
    /// Whether each pull request read in full has the paths its head
    /// changes read too (ADR-0063).
    pub files: bool,
}

/// What a page names its pull requests by: the project they are shown as,
/// the repository's full path, and who may see it where that is known.
pub(crate) type Named<'a> = (&'a str, &'a str, Option<Visibility>);

/// A page of open pull requests, where the next begins, and the forge's
/// id of their repository and who may see it, where the page names them.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Page {
    pub(crate) pulls: Vec<Pull>,
    pub(crate) next: Option<String>,
    pub(crate) repository: Option<String>,
    pub(crate) visibility: Option<Visibility>,
}

/// Whether a listing on `forge` has a page after `page`: GitHub and GitLab
/// fill every page but the last, and Forgejo's ends at an empty one.
fn listed_more(forge: Forge, page: &Page) -> bool {
    match forge {
        Forge::GitHub | Forge::GitLab => page.pulls.len() >= PAGE,
        Forge::Forgejo => !page.pulls.is_empty(),
    }
}

impl Opened<'_> {
    /// The path it reads by.
    #[must_use]
    pub const fn read_path(&self) -> ReadPath {
        ReadPath::of(self.origin.forge(), self.token.is_some())
    }

    /// The request for the page after `after`.
    fn request(&self, after: Option<&str>, number: usize) -> Request {
        let forge = self.origin.forge();
        match self.read_path() {
            ReadPath::Graphql => authorized(
                forge,
                github::request(self.graphql, self.origin.path(), after, self.ask()),
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
        self.changed(http, pull).await
    }

    /// `pull` with the paths its head changes read, where it reads them.
    async fn changed(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        pull: &mut Pull,
    ) -> Result<(), Unpolled> {
        if self.files {
            let path = detail::files_path(self.origin.forge(), pull.number);
            let asked = self.origin.get(&path, self.token);
            let body = self.asked(http, asked, detail::FILES).await?;
            let parse = self.origin.reading().parse;
            pull.files = Some(detail::files(&body, parse).map_err(Unpolled::unreadable)?);
        }
        Ok(())
    }

    /// How its GraphQL requests read their answers.
    const fn ask(&self) -> github::Ask {
        github::Ask {
            parse: self.origin.reading().parse,
            files: self.files,
        }
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
            let mut page = self.read(&body).map_err(Unpolled::unreadable)?;
            let more = if self.read_path() == ReadPath::Graphql {
                page.next.is_some()
            } else {
                listed_more(self.origin.forge(), &page)
            };
            let states = self.stated && self.token.is_none() && self.visibility.is_none();
            if let Some(stated) = page.visibility.filter(|_| states) {
                for pull in &mut page.pulls {
                    pull.visibility = stated;
                }
            }
            pulls.extend(page.pulls);
            after = page.next;
            if !more {
                break;
            }
        }
        pulls.truncate(self.most_open);
        match self.read_path() {
            ReadPath::Detailed => {
                for (read, pull) in pulls.iter_mut().enumerate() {
                    if read < self.most_pulls {
                        self.detailed(http, pull).await?;
                    } else {
                        pull.unread = true;
                    }
                }
            }
            ReadPath::Listing => {
                for pull in pulls.iter_mut().take(self.most_pulls) {
                    self.changed(http, pull).await?;
                }
            }
            ReadPath::Graphql => {}
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

impl Queued {
    /// How its open pull requests are listed, as `asked`.
    const fn asking(&self, asked: Asked<'_>) -> Opened<'_> {
        Opened {
            origin: &self.origin,
            token: self.token.as_ref(),
            project: self.project.as_str(),
            graphql: self.graphql.as_str(),
            visibility: None,
            most_open: asked.most.open,
            most_pulls: asked.most.pulls,
            stated: asked.stated,
            files: asked.files,
        }
    }
}

impl herald_kinds::Queue for Queued {
    fn scoped(&self) -> bool {
        false
    }

    fn holds(&self, repository: &str) -> bool {
        self.origin.path().eq_ignore_ascii_case(repository)
    }

    fn planned<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        (_, head, check): (&'a str, &'a str, &'a str),
        most: usize,
    ) -> herald_ports::Boxed<'a, Result<herald_kinds::Plan, Unpolled>> {
        Box::pin(async move {
            self.asking(Asked::default())
                .planned(&Erased(http), (head, check), most)
                .await
        })
    }

    fn opened<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        asked: Asked<'a>,
    ) -> herald_ports::Boxed<'a, Result<Vec<Pull>, Unpolled>> {
        Box::pin(async move { self.asking(asked).pulls(&Erased(http)).await })
    }

    fn listed<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        asked: Asked<'a>,
        validators: &'a [herald_core::Validators],
    ) -> herald_ports::Boxed<'a, Result<herald_kinds::Relisted, Unpolled>> {
        Box::pin(async move { self.asking(asked).relisted(&Erased(http), validators).await })
    }

    fn completed<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        asked: Asked<'a>,
        pulls: Vec<Pull>,
    ) -> herald_ports::Boxed<'a, Result<Vec<Pull>, Unpolled>> {
        Box::pin(async move { self.asking(asked).completed(&Erased(http), pulls).await })
    }
}

/// What reading at most `most` open pull requests of a repository on
/// `forge` costs each round, `tokened` where its source reads with a token.
#[must_use]
pub fn cost(forge: Forge, tokened: bool, most: herald_kinds::Most) -> herald_kinds::QueueCost {
    let path = ReadPath::of(forge, tokened);
    herald_kinds::QueueCost {
        path: path.name(),
        requests: path.most_requests(most.open, most.pulls),
        detailed: matches!(path, ReadPath::Detailed),
        mergeability: !matches!((forge, path), (Forge::GitHub, ReadPath::Detailed)),
        scoped: false,
        files: files_cost(path, most),
    }
}

/// The most requests reading the paths each pull request's head changes
/// adds a round on `path`, reading at most `most`: none on GitHub's
/// GraphQL, which reads them with each pull request, and one for each
/// pull request read in full elsewhere (ADR-0063).
fn files_cost(path: ReadPath, most: herald_kinds::Most) -> usize {
    match path {
        ReadPath::Graphql => 0,
        ReadPath::Listing | ReadPath::Detailed => most.pulls.min(most.open),
    }
}

#[cfg(test)]
mod tests;
