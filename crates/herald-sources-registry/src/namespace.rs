//! A registry namespace's packages, each polled on its own as an owner's
//! members are: an npm scope, a Packagist vendor, a crates.io owner or a
//! Docker Hub namespace (ADR-0039).

use std::collections::BTreeMap;
use std::sync::Arc;

use herald_core::{Listed, Listing, Origin, Owner, Selection, Visibility, media, status};
use herald_kinds::{Expand, Expanded, Poll, Unlisted, Unpolled};
use herald_ports::{
    Boxed, DynHttp, Erased, Http as _, Method, Request, Response, Secret, SecretText,
};
use herald_wire::{SourceError, answered};
use jiff::SignedDuration;

use crate::kind::{Versions, bearer};
use crate::options::{CratesOwner, RegistryOptions, VendorListing};
use crate::{Package, Registry, crates_io, docker_hub, npm, packagist};

/// One page of a namespace's listing: each package's name, with who may
/// see it where the listing says, and whether a next page follows.
pub(crate) struct Page {
    pub(crate) packages: Vec<(String, Option<Visibility>)>,
    pub(crate) more: bool,
}

/// A registry namespace's packages, each taken by the selection polled on
/// its own.
#[derive(Debug)]
pub(crate) struct Namespaced {
    registry: Registry,
    namespace: String,
    asker: Package,
    bases: Vec<String>,
    listing_api: Option<String>,
    selection: Selection,
    options: RegistryOptions,
    token: Option<Secret>,
}

impl Namespaced {
    /// The packages of `namespace` on `registry`, taken as `options` say,
    /// asked with `token`.
    pub(crate) fn new(
        registry: Registry,
        namespace: &str,
        options: &RegistryOptions,
        token: Option<Secret>,
    ) -> Self {
        let asker = Package::new(registry, namespace, options);
        let listing_api = (registry == Registry::Packagist
            && options.listing() == VendorListing::Packagist)
            .then(|| {
                options
                    .listing_api
                    .as_deref()
                    .unwrap_or(packagist::LISTING)
                    .trim_end_matches('/')
                    .to_owned()
            });
        Self {
            registry,
            namespace: namespace.to_owned(),
            bases: listing_api.as_ref().map_or_else(Vec::new, |listing| {
                vec![asker.api().to_owned(), listing.clone()]
            }),
            listing_api,
            asker,
            selection: options.selection().unwrap_or_default(),
            options: options.clone(),
            token,
        }
    }

    /// The URL of the listing's page `number`, for the crates.io owner
    /// whose id is `id`.
    fn page_url(&self, number: usize, id: Option<u64>) -> String {
        let api = self.asker.api();
        let page = self.options.listed_page(self.registry);
        match self.registry {
            Registry::Npm => format!("{api}{}", npm::org_path(&self.namespace)),
            Registry::Packagist => match &self.listing_api {
                Some(listing) => format!("{listing}{}", packagist::vendor_path(&self.namespace)),
                None => format!("{api}{}", packagist::COMPOSER),
            },
            Registry::CratesIo => format!(
                "{api}{}",
                crates_io::crates_path(self.owner(), id.unwrap_or_default(), page, number)
            ),
            Registry::DockerHub => format!(
                "{api}{}",
                docker_hub::repositories_path(&self.namespace, page, number)
            ),
        }
    }

    /// The crates.io owner the source names.
    fn owner(&self) -> CratesOwner<'_> {
        CratesOwner::of(&self.namespace).unwrap_or(CratesOwner::User(&self.namespace))
    }

    /// A request for `url`, authorised by `bearer` where it is asked of the
    /// registry's own API, conditional on `etag`.
    fn request(&self, url: String, bearer: Option<&Secret>, etag: Option<&str>) -> Request {
        let request = Request::new(Method::Get, SecretText::plain(url))
            .at_most(self.options.reading(self.registry).parse)
            .header("Accept", SecretText::plain(media::JSON))
            .identified();
        let request = match bearer.filter(|_| self.listing_api.is_none()) {
            Some(token) => self.asker.authorised(request, token),
            None => request,
        };
        request.conditional(etag, None)
    }

    /// What a page of the listing holds.
    fn page(&self, response: &Response) -> Result<Page, Unlisted> {
        let limit = self.options.reading(self.registry).parse;
        let body = &response.body;
        match self.registry {
            Registry::Npm => npm::org_packages(limit, &self.namespace, self.options.unscoped, body),
            Registry::Packagist => match self.listing_api {
                Some(_) => packagist::vendor_packages(limit, &self.namespace, body),
                None => packagist::composer_packages(limit, &self.namespace, body),
            },
            Registry::CratesIo => crates_io::crates(limit, body),
            Registry::DockerHub => docker_hub::repositories(limit, &self.namespace, body),
        }
        .map_err(failed)
    }

    /// The id crates.io gives the owner the source names, asked with
    /// `http`; none on other registries.
    async fn owner_id(&self, http: &dyn DynHttp) -> Result<Option<u64>, Unlisted> {
        if self.registry != Registry::CratesIo {
            return Ok(None);
        }
        let url = format!(
            "{}{}",
            self.asker.api(),
            crates_io::owner_path(&self.owner())
        );
        let answer = Erased(http)
            .send(self.request(url, None, None))
            .await
            .map_err(|error| Unlisted::Failed(Unpolled::unreachable(error)))?;
        answered(crates_io::OWNER, &answer).map_err(failed)?;
        crates_io::owner_id(self.options.reading(self.registry).parse, &answer.body)
            .map(Some)
            .map_err(failed)
    }

    /// The package `name`, as a member.
    fn member_of(&self, name: String, visibility: Option<Visibility>) -> Listed {
        let short = name
            .rsplit_once('/')
            .map_or(name.as_str(), |(_, short)| short)
            .to_owned();
        Listed {
            id: name.clone(),
            name: short,
            path: name,
            topics: Vec::new(),
            properties: BTreeMap::new(),
            visibility: visibility.unwrap_or_else(|| self.options.listed_visibility()),
            archived: false,
            fork: false,
            template: false,
            language: None,
            linked: None,
        }
    }

    async fn list(
        &self,
        http: &dyn DynHttp,
        known: Option<&Listing>,
    ) -> Result<Expanded, Unlisted> {
        let fingerprint = self.fingerprint();
        let etag = known
            .filter(|known| known.fingerprint == fingerprint && known.pages.len() == 1)
            .filter(|_| !self.registry.pages_its_listing())
            .and_then(|known| known.pages.first().cloned().flatten());
        let bearer = bearer(&self.asker, http, self.token.as_ref())
            .await
            .map_err(Unlisted::Failed)?;
        let id = self.owner_id(http).await?;
        let most_listed = self.options.most_listed();
        let mut pages = Vec::new();
        let mut listed = Vec::new();
        loop {
            let url = self.page_url(pages.len() + 1, id);
            let asked = etag.as_deref().filter(|_| pages.is_empty());
            let answer = Erased(http)
                .send(self.request(url, bearer.as_ref(), asked))
                .await
                .map_err(|error| Unlisted::Failed(Unpolled::unreachable(error)))?;
            if asked.is_some() && answer.status == status::NOT_MODIFIED {
                return Ok(Expanded::Unchanged);
            }
            self.answered(&answer)?;
            let page = self.page(&answer)?;
            if let Some((name, _)) = page
                .packages
                .iter()
                .find(|(name, _)| !self.registry.writes_package(name))
            {
                return Err(failed(SourceError::Unreadable {
                    what: self.registry.listing(),
                    reason: format!(
                        "it names \"{name}\", which is not a package's name: {}, of letters, digits, '.', '_' and '-'",
                        self.registry.shape()
                    ),
                }));
            }
            pages.push(answer.header("etag").map(ToOwned::to_owned));
            let more = page.more && !page.packages.is_empty();
            listed.extend(
                page.packages
                    .into_iter()
                    .map(|(name, visibility)| self.member_of(name, visibility)),
            );
            if listed.len() > most_listed.get() {
                return Err(Unlisted::Over(format!(
                    "{} lists more than {most_listed} packages, most_listed; raise it, or name a narrower {}",
                    self.namespace,
                    self.registry.namespace_key()
                )));
            }
            if !more {
                break;
            }
        }
        let members: Vec<Listed> = listed
            .into_iter()
            .filter(|package| self.selection.takes(package))
            .collect();
        let most_members = self.options.most_members();
        if members.len() > most_members.get() {
            return Err(Unlisted::Over(format!(
                "{} packages of {} are taken, more than most_members, {most_members}; raise it, or narrow include and exclude",
                members.len(),
                self.namespace
            )));
        }
        Ok(Expanded::Listed {
            members,
            owner: Owner::Organization,
            pages,
            file: None,
            missing: Vec::new(),
        })
    }

    /// Whether `answer` is a listing: npm's refusal to list an
    /// organization says how to list it.
    fn answered(&self, answer: &Response) -> Result<(), Unlisted> {
        let refused = [status::UNAUTHORIZED, status::FORBIDDEN, status::NOT_FOUND];
        if self.registry == Registry::Npm && refused.contains(&answer.status) {
            return Err(Unlisted::Failed(Unpolled::Unreadable(format!(
                "npm answered {} to the listing of the organization {}; give a token that may read it, or name its packages one by one with package",
                answer.status, self.namespace
            ))));
        }
        answered(self.registry.listing(), answer).map_err(failed)
    }
}

fn failed(error: SourceError) -> Unlisted {
    Unlisted::Failed(Unpolled::unreadable(error))
}

impl Expand for Namespaced {
    fn api(&self) -> &str {
        self.asker.api()
    }

    fn bases(&self) -> &[String] {
        &self.bases
    }

    fn owner_key(&self) -> &'static str {
        self.registry.namespace_key()
    }

    fn member_nouns(&self) -> (&'static str, &'static str) {
        ("package", "packages")
    }

    fn keep_members(&self) -> SignedDuration {
        self.options.keep_members()
    }

    fn remembered(&self) -> usize {
        self.options.reading(self.registry).remembered
    }

    fn listed<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        known: Option<&'a Listing>,
    ) -> Boxed<'a, Result<Expanded, Unlisted>> {
        Box::pin(self.list(http, known))
    }

    fn member(&self, package: &Listed) -> Arc<dyn Poll> {
        Arc::new(Versions {
            package: Package::new(self.registry, &package.path, &self.options),
            token: self.token.clone(),
        })
    }

    fn origin(&self, package: &Listed) -> Origin {
        Origin {
            owner: self.namespace.trim_start_matches('@').to_owned(),
            visibility: package.visibility,
            ..Package::new(self.registry, &package.path, &self.options).origin()
        }
    }

    fn project(&self, package: &Listed) -> Result<String, String> {
        Ok(Package::new(self.registry, &package.path, &self.options)
            .project()
            .to_owned())
    }

    fn takes(&self, package: &Listed) -> bool {
        self.selection.takes(package)
    }

    fn fingerprint(&self) -> String {
        herald_core::digest(&format!(
            "{:?}\n{}\n{}\n{:?}\n{:?}\n{:?}\n{}\n{:?}\n{}",
            self.registry,
            self.asker.api(),
            self.namespace,
            self.listing_api,
            self.selection,
            self.options.listing(),
            self.options.unscoped,
            self.options.listed_visibility(),
            self.options.listed_page(self.registry)
        ))
    }
}

#[cfg(test)]
mod tests;
