use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::Arc;

use herald_core::{Listed, Listing, Owner, Selection, Visibility, status, unreserved};
use herald_kinds::{Expand, Expanded, Poll, Unlisted, Unpolled};
use herald_ports::{Boxed, DynHttp, Erased, Http as _, Method, Request, Response, SecretText};
use herald_wire::{Reading, SourceError, answered};
use jiff::SignedDuration;
use serde::Deserialize;

use crate::forge::{api_of, authorized};
use crate::kind::Releases;
use crate::{Forge, Origin, SourceOptions, Token};

/// An owner's repositories on a forge, each taken by the selection polled on
/// its own (ADR-0029).
#[derive(Debug)]
pub(crate) struct Owned {
    forge: Forge,
    api: String,
    owner: String,
    selection: Selection,
    most_listed: NonZeroUsize,
    most_members: NonZeroUsize,
    keep_members: SignedDuration,
    subgroups: bool,
    page: NonZeroUsize,
    options: SourceOptions,
    token: Option<Token>,
}

impl Owned {
    /// The repositories of `owner` on `forge`, taken as `options` say,
    /// asked with `token`.
    pub(crate) fn new(
        forge: Forge,
        owner: &str,
        options: &SourceOptions,
        token: Option<Token>,
    ) -> Self {
        Self {
            forge,
            api: api_of(forge, options),
            owner: owner.to_owned(),
            selection: options.selection().unwrap_or_default(),
            most_listed: options.most_listed(),
            most_members: options.most_members(),
            keep_members: options.keep_members(),
            subgroups: options.subgroups(),
            page: options.listed_page.unwrap_or(forge.api().listed_page),
            options: options.clone(),
            token,
        }
    }

    fn reading(&self) -> Reading {
        self.options.reading()
    }

    /// The request for page `number` of the owner's repositories, as an
    /// `owner`, conditional on `etag` where there is one.
    pub(crate) fn page_request(&self, owner: Owner, number: usize, etag: Option<&str>) -> Request {
        let (api, name, page) = (&self.api, &self.owner, self.page);
        let sized = self.forge.api().page;
        let url = match (self.forge, owner) {
            (Forge::GitHub, Owner::Organization) => format!(
                "{api}/orgs/{name}/repos?type=all&sort=full_name&direction=asc&{sized}={page}&page={number}"
            ),
            (Forge::GitHub, Owner::User) => format!(
                "{api}/users/{name}/repos?type=owner&sort=full_name&direction=asc&{sized}={page}&page={number}"
            ),
            (Forge::GitLab, Owner::Organization) => format!(
                "{api}/groups/{}/projects?include_subgroups={}&with_shared=false&order_by=id&sort=asc&{sized}={page}&page={number}",
                unreserved(name),
                self.subgroups
            ),
            (Forge::GitLab, Owner::User) => format!(
                "{api}/users/{name}/projects?order_by=id&sort=asc&{sized}={page}&page={number}"
            ),
            (Forge::Forgejo, Owner::Organization) => {
                format!("{api}/orgs/{name}/repos?{sized}={page}&page={number}")
            }
            (Forge::Forgejo, Owner::User) => {
                format!("{api}/users/{name}/repos?{sized}={page}&page={number}")
            }
        };
        let request =
            Request::new(Method::Get, SecretText::plain(url)).at_most(self.reading().parse);
        authorized(
            self.forge,
            request.conditional(etag, None),
            self.token.as_ref(),
        )
    }

    /// The repositories one page of the listing names.
    ///
    /// # Errors
    ///
    /// When the forge did not answer with repositories.
    pub(crate) fn page(&self, response: &Response) -> Result<Vec<Listed>, SourceError> {
        let what = self.forge.api().listing;
        answered(what, response)?;
        let limit = self.reading().parse;
        Ok(match self.forge {
            Forge::GitHub => herald_wire::read::<Vec<GitHub>>(what, limit, &response.body)?
                .into_iter()
                .map(Listed::from)
                .collect(),
            Forge::GitLab => herald_wire::read::<Vec<GitLab>>(what, limit, &response.body)?
                .into_iter()
                .map(Listed::from)
                .collect(),
            Forge::Forgejo => herald_wire::read::<Vec<Forgejo>>(what, limit, &response.body)?
                .into_iter()
                .map(Listed::from)
                .collect(),
        })
    }

    async fn list(
        &self,
        http: &dyn DynHttp,
        known: Option<&Listing>,
    ) -> Result<Expanded, Unlisted> {
        let http = Erased(http);
        let fingerprint = self.fingerprint();
        let known = known.filter(|known| known.fingerprint == fingerprint);
        let mut owner = known.map_or(Owner::Organization, |known| known.owner);
        let known_pages = known.map_or(&[][..], |known| known.pages.as_slice());
        let mut conditional = !known_pages.is_empty() && known_pages.iter().all(Option::is_some);
        let mut pages = Vec::new();
        let mut listed = Vec::new();
        loop {
            let number = pages.len() + 1;
            let etag = known_pages
                .get(pages.len())
                .filter(|_| conditional)
                .cloned()
                .flatten();
            let answer = http
                .send(self.page_request(owner, number, etag.as_deref()))
                .await
                .map_err(|error| Unlisted::Failed(Unpolled::unreachable(error)))?;
            if answer.status == status::NOT_FOUND
                && number == 1
                && known.is_none()
                && owner == Owner::Organization
            {
                owner = Owner::User;
                continue;
            }
            if etag.is_some() && Forge::unchanged(&answer) {
                pages.push(etag);
                continue;
            }
            let page = self.page(&answer).map_err(failed)?;
            if conditional {
                if pages.len() == known_pages.len() && page.is_empty() {
                    return Ok(Expanded::Unchanged);
                }
                conditional = false;
                pages.clear();
                continue;
            }
            let more = next(&answer) && !page.is_empty();
            pages.push(answer.header("etag").map(ToOwned::to_owned));
            listed.extend(page);
            if listed.len() > self.most_listed.get() {
                return Err(Unlisted::Over(format!(
                    "{} lists more than {} repositories, most_listed; raise it, or name a narrower owner",
                    self.owner, self.most_listed
                )));
            }
            if !more {
                break;
            }
        }
        let members: Vec<Listed> = listed
            .into_iter()
            .filter(|repository| self.selection.takes(repository))
            .collect();
        if members.len() > self.most_members.get() {
            return Err(Unlisted::Over(format!(
                "{} repositories of {} are taken, more than most_members, {}; raise it, or narrow the filters",
                members.len(),
                self.owner,
                self.most_members
            )));
        }
        Ok(Expanded::Listed {
            members,
            owner,
            pages,
        })
    }
}

fn failed(error: SourceError) -> Unlisted {
    Unlisted::Failed(Unpolled::unreadable(error))
}

/// Whether the forge's answer names a next page.
fn next(response: &Response) -> bool {
    response.header("link").is_some_and(|link| {
        link.split(',').any(|part| {
            part.split(';')
                .skip(1)
                .any(|param| param.trim() == "rel=\"next\"")
        })
    })
}

impl Expand for Owned {
    fn api(&self) -> &str {
        &self.api
    }

    fn keep_members(&self) -> SignedDuration {
        self.keep_members
    }

    fn listed<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        known: Option<&'a Listing>,
    ) -> Boxed<'a, Result<Expanded, Unlisted>> {
        Box::pin(self.list(http, known))
    }

    fn takes(&self, repository: &Listed) -> bool {
        self.selection.takes(repository)
    }

    fn fingerprint(&self) -> String {
        herald_core::digest(&format!(
            "{:?}\n{}\n{}\n{}\n{:?}",
            self.forge, self.api, self.owner, self.subgroups, self.selection
        ))
    }

    fn member(&self, repository: &Listed) -> Arc<dyn Poll> {
        Arc::new(Releases {
            origin: Origin::new(self.forge, &self.options, &repository.path),
            token: self.token.clone(),
        })
    }
}

/// The visibility a forge names, as GitHub's and GitLab's listings spell
/// it; any other name is private, so an unknown one is never announced to
/// a public place.
fn named(name: &str) -> Visibility {
    match name {
        "public" => Visibility::Public,
        "internal" => Visibility::Internal,
        _ => Visibility::Private,
    }
}

/// The visibility a repository's flags give: private unless it says it is
/// not, and internal where it says so.
const fn flagged(private: Option<bool>, internal: Option<bool>) -> Visibility {
    match (private, internal) {
        (Some(false), Some(true)) => Visibility::Internal,
        (Some(false), _) => Visibility::Public,
        _ => Visibility::Private,
    }
}

/// The visibility GitLab's numeric level gives: 20 public, 10 internal,
/// and private otherwise.
const fn levelled(level: Option<u8>) -> Visibility {
    match level {
        Some(20) => Visibility::Public,
        Some(10) => Visibility::Internal,
        _ => Visibility::Private,
    }
}

/// A custom property's value, as GitHub names it.
#[derive(Deserialize)]
#[serde(untagged)]
enum Property {
    One(String),
    Many(Vec<String>),
}

/// A repository, as GitHub lists it.
#[derive(Deserialize)]
struct GitHub {
    id: u64,
    name: String,
    full_name: String,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    custom_properties: BTreeMap<String, Option<Property>>,
    visibility: Option<String>,
    private: Option<bool>,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    fork: bool,
    #[serde(default)]
    is_template: bool,
    language: Option<String>,
}

impl From<GitHub> for Listed {
    fn from(repository: GitHub) -> Self {
        let visibility = repository
            .visibility
            .as_deref()
            .map_or_else(|| flagged(repository.private, None), named);
        Self {
            id: repository.id.to_string(),
            name: repository.name,
            path: repository.full_name,
            topics: repository.topics,
            properties: repository
                .custom_properties
                .into_iter()
                .filter_map(|(name, value)| {
                    value.map(|value| {
                        let values = match value {
                            Property::One(value) => vec![value],
                            Property::Many(values) => values,
                        };
                        (name, values)
                    })
                })
                .collect(),
            visibility,
            archived: repository.archived,
            fork: repository.fork,
            template: repository.is_template,
            language: repository.language,
        }
    }
}

/// A project, as GitLab lists it.
#[derive(Deserialize)]
struct GitLab {
    id: u64,
    path: String,
    path_with_namespace: String,
    #[serde(default)]
    topics: Vec<String>,
    visibility: Option<String>,
    visibility_level: Option<u8>,
    #[serde(default)]
    archived: bool,
    forked_from_project: Option<serde_json::Value>,
}

impl From<GitLab> for Listed {
    fn from(project: GitLab) -> Self {
        Self {
            id: project.id.to_string(),
            name: project.path,
            path: project.path_with_namespace,
            topics: project.topics,
            properties: BTreeMap::new(),
            visibility: project
                .visibility
                .as_deref()
                .map_or_else(|| levelled(project.visibility_level), named),
            archived: project.archived,
            fork: project.forked_from_project.is_some(),
            template: false,
            language: None,
        }
    }
}

/// A repository, as Forgejo and Gitea list it.
#[derive(Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the struct reads Forgejo's answer, which names each flag"
)]
struct Forgejo {
    id: u64,
    name: String,
    full_name: String,
    #[serde(default)]
    topics: Vec<String>,
    private: Option<bool>,
    #[serde(default)]
    internal: bool,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    fork: bool,
    #[serde(default)]
    template: bool,
    #[serde(default)]
    language: String,
}

impl From<Forgejo> for Listed {
    fn from(repository: Forgejo) -> Self {
        let visibility = flagged(repository.private, Some(repository.internal));
        Self {
            id: repository.id.to_string(),
            name: repository.name,
            path: repository.full_name,
            topics: repository.topics,
            properties: BTreeMap::new(),
            visibility,
            archived: repository.archived,
            fork: repository.fork,
            template: repository.template,
            language: Some(repository.language).filter(|language| !language.is_empty()),
        }
    }
}

#[cfg(test)]
mod tests;
