//! A reconcile's listing of a repository's open pull requests, each page
//! asked conditional on what it was last answered with, and each changed
//! pull request read in full (ADR-0061).

use herald_core::{Pull, Validators};
use herald_kinds::{Relisted, Unpolled};
use herald_ports::{DynHttp, Erased, Http as _, Request, Response};
use herald_wire::answered;

use super::{PAGE, Page, ReadPath, github, gitlab, listed_more, listing};
use crate::Forge;
use crate::forge::{authorized, validators};

use super::Opened;

impl Opened<'_> {
    /// The path of the `number`th page of the listing a reconcile reads.
    fn listing_path(&self, number: usize) -> String {
        match self.origin.forge() {
            Forge::GitLab => gitlab::path(number),
            forge => listing::path(forge, number),
        }
    }

    /// What that listing is read as.
    const fn listing_what(&self) -> &'static str {
        match self.origin.forge() {
            Forge::GitHub => listing::GITHUB,
            Forge::GitLab => gitlab::WHAT,
            Forge::Forgejo => listing::FORGEJO,
        }
    }

    /// The page of that listing `answer` holds.
    fn listing_page(&self, answer: &Response) -> Result<Page, Unpolled> {
        let what = self.listing_what();
        answered(what, answer).map_err(Unpolled::unreadable)?;
        let parse = self.origin.reading().parse;
        let named = (self.project, self.origin.path(), self.visibility);
        match self.origin.forge() {
            Forge::GitLab => gitlab::page(&answer.body, parse, named),
            Forge::GitHub | Forge::Forgejo => listing::page(what, &answer.body, parse, named),
        }
        .map_err(Unpolled::unreadable)
    }

    /// Its open pull requests, at most `most_open`, each read as far as its
    /// listing reads it: unchanged where every page `validators` holds is,
    /// and else listed whole, a page already answered in full read once.
    ///
    /// # Errors
    ///
    /// Where a page could not be asked or read.
    pub async fn relisted(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        validators_kept: &[Validators],
    ) -> Result<Relisted, Unpolled> {
        let forge = self.origin.forge();
        let listing = Paged {
            pages: self.most_open.div_ceil(PAGE),
            request: &|number| self.origin.get(&self.listing_path(number), self.token),
            read: &|answer| {
                let page = self.listing_page(answer)?;
                let more = listed_more(forge, &page);
                Ok((page, more))
            },
        };
        let Some((pages, validators)) = listing.relisted(http, validators_kept).await? else {
            return Ok(Relisted::Unchanged);
        };
        let mut pulls = Vec::new();
        let mut repository = None;
        let mut visibility = None;
        for page in pages {
            repository = repository.or(page.repository);
            visibility = visibility.or(page.visibility);
            pulls.extend(page.pulls);
        }
        pulls.truncate(self.most_open);
        Ok(Relisted::Listed {
            pulls,
            repository,
            visibility,
            validators,
        })
    }

    /// `pulls`, as [`Opened::relisted`] found them, each read in full: as
    /// listed on GitLab, whose listing reads them so; through GitHub's
    /// GraphQL a page of them a request; else each one's reviews and
    /// checks. One GraphQL holds no pull request for is left out.
    ///
    /// # Errors
    ///
    /// Where one could not be asked or read.
    pub async fn completed(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        mut pulls: Vec<Pull>,
    ) -> Result<Vec<Pull>, Unpolled> {
        match self.read_path() {
            ReadPath::Listing => {
                for pull in pulls.iter_mut().take(self.most_pulls) {
                    self.changed(http, pull).await?;
                }
                Ok(pulls)
            }
            ReadPath::Detailed => {
                for pull in &mut pulls {
                    self.detailed(http, pull).await?;
                }
                Ok(pulls)
            }
            ReadPath::Graphql => {
                let parse = self.origin.reading().parse;
                let named = (self.project, self.origin.path(), self.visibility);
                let mut read = Vec::new();
                for chunk in pulls.chunks(PAGE) {
                    let numbers: Vec<u64> = chunk.iter().map(|pull| pull.number).collect();
                    let request = authorized(
                        self.origin.forge(),
                        github::numbered_request(
                            self.graphql,
                            self.origin.path(),
                            &numbers,
                            self.ask(),
                        ),
                        self.token,
                    );
                    let body = self.asked(http, request, github::WHAT).await?;
                    read.extend(
                        github::numbered(&body, parse, named).map_err(Unpolled::unreadable)?,
                    );
                }
                Ok(read)
            }
        }
    }
}

/// A listing a reconcile asks a page at a time, each conditional on what
/// it was last answered with.
pub(super) struct Paged<'a, T> {
    /// The most pages it reads.
    pub(super) pages: usize,
    /// The request for the `number`th page.
    pub(super) request: &'a (dyn Fn(usize) -> Result<Request, Unpolled> + Sync),
    /// The page an answer holds, and whether another follows it.
    pub(super) read: &'a PageRead<'a, T>,
}

/// What reads a page of a listing a reconcile asks: the page an answer
/// holds, and whether another follows it.
pub(super) type PageRead<'a, T> = dyn Fn(&Response) -> Result<(T, bool), Unpolled> + Sync + 'a;

impl<T> Paged<'_, T> {
    /// Its pages, asked through `http`: none where every page `kept` holds
    /// validators for is unchanged, and else each page with the validators
    /// it was answered with, a page already answered in full read once.
    ///
    /// # Errors
    ///
    /// Where a page could not be asked or read.
    pub(super) async fn relisted(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        kept: &[Validators],
    ) -> Result<Option<(Vec<T>, Vec<Validators>)>, Unpolled> {
        let mut changed = None;
        for (index, validators) in kept.iter().take(self.pages).enumerate() {
            let request = (self.request)(index + 1)?.conditional(
                validators.etag.as_deref(),
                validators.last_modified.as_deref(),
            );
            let answer = http.send(request).await.map_err(Unpolled::unreachable)?;
            if !Forge::unchanged(&answer) {
                changed = Some((index + 1, answer));
                break;
            }
        }
        if changed.is_none() && !kept.is_empty() {
            return Ok(None);
        }
        let mut pages = Vec::new();
        let mut answered = Vec::new();
        for number in 1..=self.pages {
            let answer = match changed.take_if(|(at, _)| *at == number) {
                Some((_, answer)) => answer,
                None => http
                    .send((self.request)(number)?)
                    .await
                    .map_err(Unpolled::unreachable)?,
            };
            let (page, more) = (self.read)(&answer)?;
            answered.push(validators(&answer));
            pages.push(page);
            if !more {
                break;
            }
        }
        Ok(Some((pages, answered)))
    }
}

#[cfg(test)]
mod tests;
