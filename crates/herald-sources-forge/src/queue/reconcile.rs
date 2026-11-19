//! A reconcile's listing of a repository's open pull requests, each page
//! asked conditional on what it was last answered with, and each changed
//! pull request read in full (ADR-0061).

use herald_core::{Pull, Validators};
use herald_kinds::{Relisted, Unpolled};
use herald_ports::{DynHttp, Erased, Http as _, Response};
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

    /// The answer to the `number`th page of the listing, asked conditional
    /// on `kept` where given.
    async fn listing_answer(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        number: usize,
        kept: Option<&Validators>,
    ) -> Result<Response, Unpolled> {
        let request = self.origin.get(&self.listing_path(number), self.token);
        let request = match kept {
            Some(kept) => request.conditional(kept.etag.as_deref(), kept.last_modified.as_deref()),
            None => request,
        };
        http.send(request).await.map_err(Unpolled::unreachable)
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
        let pages = self.most_open.div_ceil(PAGE);
        let mut changed = None;
        for (index, kept) in validators_kept.iter().take(pages).enumerate() {
            let answer = self.listing_answer(http, index + 1, Some(kept)).await?;
            if !Forge::unchanged(&answer) {
                changed = Some((index + 1, answer));
                break;
            }
        }
        if changed.is_none() && !validators_kept.is_empty() {
            return Ok(Relisted::Unchanged);
        }
        let mut pulls = Vec::new();
        let mut repository = None;
        let mut visibility = None;
        let mut kept = Vec::new();
        for number in 1..=pages {
            let answer = match changed.take_if(|(at, _)| *at == number) {
                Some((_, answer)) => answer,
                None => self.listing_answer(http, number, None).await?,
            };
            let page = self.listing_page(&answer)?;
            kept.push(validators(&answer));
            let more = listed_more(self.origin.forge(), &page);
            repository = repository.or(page.repository);
            visibility = visibility.or(page.visibility);
            pulls.extend(page.pulls);
            if !more {
                break;
            }
        }
        pulls.truncate(self.most_open);
        Ok(Relisted::Listed {
            pulls,
            repository,
            visibility,
            validators: kept,
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
            ReadPath::Listing => Ok(pulls),
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
                        github::numbered_request(self.graphql, self.origin.path(), &numbers, parse),
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

#[cfg(test)]
mod tests;
