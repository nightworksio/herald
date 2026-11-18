//! A repository's open pull requests as a view reads them: whose turn each
//! is, from its reviews, checks and mergeability.

use herald_core::{Pull, Visibility};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Http as _, Request};
use herald_wire::answered;

use crate::forge::authorized;
use crate::{Forge, Origin, Token};

mod forgejo;
mod github;
mod gitlab;

/// The most open pull requests one page lists, on every forge.
pub const PAGE: usize = 100;

/// The most open pull requests a repository's listing reads by default.
pub const MOST_PULLS: usize = 500;

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
    pub most: usize,
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
    /// The request for the page after `after`.
    fn request(&self, after: Option<&str>, number: usize) -> Request {
        let parse = self.origin.reading().parse;
        match self.origin.forge() {
            Forge::GitHub => authorized(
                Forge::GitHub,
                github::request(self.graphql, self.origin.path(), after, parse),
                self.token,
            ),
            Forge::GitLab => self.origin.get(&gitlab::path(number), self.token),
            Forge::Forgejo => self.origin.get(&forgejo::path(number), self.token),
        }
    }

    /// The page `body` holds.
    fn read(&self, body: &[u8]) -> Result<Page, herald_wire::SourceError> {
        let parse = self.origin.reading().parse;
        let named = (self.project, self.origin.path(), self.visibility);
        match self.origin.forge() {
            Forge::GitHub => github::page(body, parse, named),
            Forge::GitLab => gitlab::page(body, parse, named),
            Forge::Forgejo => forgejo::page(body, parse, named),
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
        for number in 1..=self.most.div_ceil(PAGE) {
            let answer = http
                .send(self.request(after.as_deref(), number))
                .await
                .map_err(Unpolled::unreachable)?;
            answered(what, &answer).map_err(Unpolled::unreadable)?;
            let page = self.read(&answer.body).map_err(Unpolled::unreadable)?;
            let more = match self.origin.forge() {
                Forge::GitHub => page.next.is_some(),
                Forge::GitLab => page.pulls.len() >= PAGE,
                Forge::Forgejo => !page.pulls.is_empty(),
            };
            pulls.extend(page.pulls);
            after = page.next;
            if !more {
                break;
            }
        }
        pulls.truncate(self.most);
        Ok(pulls)
    }

    /// What its listing is read as.
    const fn what(&self) -> &'static str {
        match self.origin.forge() {
            Forge::GitHub => github::WHAT,
            Forge::GitLab => gitlab::WHAT,
            Forge::Forgejo => forgejo::WHAT,
        }
    }
}

#[cfg(test)]
mod tests;
