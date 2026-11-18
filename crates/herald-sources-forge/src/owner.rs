use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::Arc;

use herald_core::{
    Cursor, EventsRead, Glob, Listed, Listing, Owner, Polled, ProjectName, Selection, Signal,
    Visibility, status, unreserved,
};
use herald_kinds::{
    Changed, Expand, Expanded, Poll, Registered, Registering, Rounded, Unlisted, Unpolled,
};
use herald_ports::{
    Boxed, DynHttp, Erased, Http as _, Method, Request, Response, Secret, SecretText,
};
use herald_wire::{Reading, SourceError, answered};
use jiff::SignedDuration;
use serde::Deserialize;

use crate::forge::{api_of, authorized};
use crate::graphql::{Asking, polled_together};
use crate::kind::projected;
use crate::packages::{self, OwnerPackages, PackageList, Packaging, merged, seen_packages};
use crate::polled::Releases;
use crate::{Discovery, Forge, Members, Origin, SourceOptions, Token, Transport, events};

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
    watched: usize,
    file: Option<Filed>,
    project: Option<ProjectName>,
    projects: Vec<(Glob, ProjectName)>,
    graphql: Option<String>,
    packaging: Packaging,
}

/// A member file read through the forge each round: the repository it is
/// in, its path there, and the reference it is read at.
#[derive(Debug)]
struct Filed {
    origin: Origin,
    path: String,
    reference: Option<String>,
}

/// The member file's names, where the round read them, and its `ETag`.
struct Read {
    names: Option<Vec<String>>,
    etag: Option<String>,
}

impl Owned {
    /// The repositories of `owner` on `forge`, taken as `options` say and,
    /// where given, of the `names` its member file lists, asked with
    /// `token`.
    pub(crate) fn new(
        forge: Forge,
        (owner, names): (&str, Option<&[String]>),
        options: &SourceOptions,
        (token, watched): (Option<Token>, usize),
    ) -> Self {
        let file = match &options.members {
            Some(Members::Repository {
                repository,
                path,
                reference,
            }) => Some(Filed {
                origin: Origin::new(forge, options, repository),
                path: path.clone(),
                reference: reference.clone(),
            }),
            Some(Members::File { .. }) | None => None,
        };
        Self {
            forge,
            api: api_of(forge, options),
            owner: owner.to_owned(),
            selection: Selection {
                names: names.map(<[String]>::to_vec),
                ..options.selection().unwrap_or_default()
            },
            most_listed: options.most_listed(),
            most_members: options.most_members(),
            keep_members: options.keep_members(),
            subgroups: options.subgroups(),
            page: options.listed_page.unwrap_or(forge.api().listed_page),
            options: options.clone(),
            token,
            watched,
            file,
            project: options.project(),
            projects: options.projects(),
            graphql: options.batched(forge).then(|| {
                options
                    .graphql
                    .clone()
                    .unwrap_or_else(|| graphql_beside(forge, &api_of(forge, options)))
            }),
            packaging: Packaging::of(forge, options),
        }
    }

    /// The member file the forge serves, asked conditionally on `etag`:
    /// its names, or none where it is unchanged; nothing where the source
    /// reads no file through the forge.
    async fn read_file(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        etag: Option<&str>,
    ) -> Result<Read, Unlisted> {
        let Some(file) = &self.file else {
            return Ok(Read {
                names: None,
                etag: None,
            });
        };
        let answer = http
            .send(file.origin.file_request(
                (&file.path, file.reference.as_deref()),
                self.token.as_ref(),
                etag,
            ))
            .await
            .map_err(|error| Unlisted::Failed(Unpolled::unreachable(error)))?;
        if etag.is_some() && Forge::unchanged(&answer) {
            return Ok(Read {
                names: None,
                etag: etag.map(ToOwned::to_owned),
            });
        }
        answered(self.forge.api().file, &answer).map_err(failed)?;
        let names = crate::members::names(
            &file.path,
            &String::from_utf8_lossy(&answer.body),
            &self.options.tables,
            self.options.key(),
        )
        .map_err(|reason| Unlisted::Failed(Unpolled::Unreadable(reason)))?;
        Ok(Read {
            names: Some(names),
            etag: answer.header("etag").map(ToOwned::to_owned),
        })
    }

    /// What polls the member `repository`, its packages listed from
    /// `list`.
    fn releases(&self, repository: &Listed, list: PackageList) -> Releases {
        let signals = self.options.signals_of(&repository.name);
        Releases {
            origin: Origin::new(self.forge, &self.options, &repository.path),
            token: self.token.clone(),
            project: self.named_project(repository),
            held: self.options.held_for(&signals),
            signals,
            backfill: self.options.backfill,
            watched: self.watched,
            packaging: self.packaging.clone(),
            list,
            visibility: Some(repository.visibility),
            runs: self.options.run_choice(),
        }
    }

    /// How the packages of a repository at `origin` are asked, listed from
    /// `list`, a pre-release told by `rule`.
    fn asking<'a>(
        &'a self,
        origin: &'a Origin,
        list: &'a PackageList,
        rule: herald_core::PrereleaseRule,
    ) -> packages::Asking<'a> {
        packages::Asking {
            origin,
            token: self.token.as_ref(),
            packaging: &self.packaging,
            rule,
            list,
            repository: None,
        }
    }

    /// What the member `repository` polled through GraphQL found of its
    /// packages since `cursor`, listed from `list`, a pre-release told as
    /// `signals` say.
    async fn packaged(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (repository, list): (&Listed, &PackageList),
        (signals, cursor): (&crate::Signals, Option<&Cursor>),
    ) -> Result<packages::Packaged, Unpolled> {
        let origin = Origin::new(self.forge, &self.options, &repository.path);
        let asking = self.asking(&origin, list, signals.prerelease);
        match list {
            PackageList::Owners(owned) => {
                packages::packaged_member(http, &asking, (repository, owned), cursor).await
            }
            PackageList::Own | PackageList::Kept => Ok(packages::Packaged::kept(cursor)),
        }
    }

    /// What the member `repository`'s runs since `cursor` are, read through
    /// `http` beside what GraphQL read of it.
    async fn ran(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        repository: &Listed,
        cursor: Option<&Cursor>,
    ) -> Result<(crate::runs::Asked, crate::runs::Runs), Unpolled> {
        let id = repository.id.parse::<u64>().map_err(|error| {
            Unpolled::Unreadable(format!(
                "the repository {}'s id \"{}\" is not a number: {error}",
                repository.path, repository.id
            ))
        })?;
        let host = crate::forge::host_of(self.forge, &self.api);
        let api = herald_core::Address::parse(&self.api);
        let scheme = api.as_ref().map_or("https", herald_core::Address::scheme);
        let described = crate::forge::Repository {
            id,
            name: repository.name.clone(),
            page: Some(format!("{scheme}://{host}/{}", repository.path)),
        };
        let origin = Origin::new(self.forge, &self.options, &repository.path);
        let choice = self.options.run_choice();
        let asking = crate::runs::Asking {
            origin: &origin,
            token: self.token.as_ref(),
            choice: &choice,
        };
        let asked = asking.asked(http, cursor).await?;
        let runs = asking.read(&asked, &described)?;
        Ok((asked, runs))
    }

    fn reading(&self) -> Reading {
        self.options.reading()
    }

    /// The project `repository` is announced as where a template names it,
    /// or why the template names none.
    fn named_project(&self, repository: &Listed) -> Option<Result<String, String>> {
        self.template_for(repository)
            .map(|_| self.project(repository))
    }

    /// The template naming `repository`'s project: the narrowest of
    /// `repositories` naming it that sets one, or else the source's.
    fn template_for(&self, repository: &Listed) -> Option<&ProjectName> {
        self.projects
            .iter()
            .find(|(glob, _)| glob.matches(&repository.name))
            .map(|(_, project)| project)
            .or(self.project.as_ref())
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
        let mut read = self
            .read_file(&http, known.and_then(|known| known.file.as_deref()))
            .await?;
        let mut conditional = read.names.is_none()
            && !known_pages.is_empty()
            && known_pages.iter().all(Option::is_some);
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
        if self.file.is_some() && read.names.is_none() {
            read = self.read_file(&http, None).await?;
        }
        let selection = match read.names {
            Some(names) => Selection {
                names: Some(names),
                ..self.selection.clone()
            },
            None => self.selection.clone(),
        };
        let missing = selection
            .names
            .iter()
            .flatten()
            .filter(|name| !listed.iter().any(|repository| repository.named(name)))
            .cloned()
            .collect();
        let members: Vec<Listed> = listed
            .into_iter()
            .filter(|repository| selection.takes(repository))
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
            file: read.etag,
            missing,
        })
    }
}

fn failed(error: SourceError) -> Unlisted {
    Unlisted::Failed(Unpolled::unreadable(error))
}

/// Whether the forge's answer names a next page.
pub(crate) fn next(response: &Response) -> bool {
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

    fn standing(&self, repository: &Listed) -> Option<herald_kinds::Standing> {
        Some(herald_kinds::Standing::new(
            &self.api,
            &repository.path,
            false,
        ))
    }

    fn keep_members(&self) -> SignedDuration {
        self.keep_members
    }

    fn remembered(&self) -> usize {
        self.reading().remembered
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
            "{:?}\n{}\n{}\n{}\n{:?}\n{:?}\n{:?}\n{}",
            self.forge,
            self.api,
            self.owner,
            self.subgroups,
            self.selection,
            self.file,
            self.options.tables,
            self.options.key()
        ))
    }

    fn pushed(&self) -> bool {
        self.options.discovery() == Discovery::Push
    }

    fn registering(&self) -> Registering {
        self.options.registering()
    }

    fn register<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        repository: &'a Listed,
        hook: (&'a str, &'a Secret),
    ) -> Boxed<'a, Result<Registered, Unpolled>> {
        Box::pin(crate::hooks::registered(
            http,
            (&self.api, self.token.as_ref()),
            (
                &repository.id,
                self.options.signals_of(&repository.name).takes(Signal::Run),
            ),
            hook,
        ))
    }

    fn hooked<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        repository: &'a Listed,
        url: &'a str,
    ) -> Boxed<'a, Result<bool, Unpolled>> {
        Box::pin(crate::hooks::found(
            http,
            (&self.api, self.token.as_ref()),
            &repository.id,
            url,
        ))
    }

    fn may_register<'a>(&'a self, http: &'a dyn DynHttp) -> Boxed<'a, Result<(), String>> {
        Box::pin(crate::hooks::may_register(
            http,
            (&self.api, self.token.as_ref()),
        ))
    }

    fn changed<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        owner: Owner,
        read: Option<&'a EventsRead>,
    ) -> Boxed<'a, Option<Result<Changed, Unpolled>>> {
        Box::pin(async move {
            let followed = match self.forge {
                Forge::GitHub => Some(Transport::Events),
                Forge::Forgejo => Some(Transport::Feed),
                Forge::GitLab => None,
            };
            if followed != Some(self.options.transport()) || self.options.may_take(Signal::Run) {
                return None;
            }
            let paged = match self.forge {
                Forge::Forgejo => (
                    self.options.feed_page().get(),
                    self.options.feed_pages().get(),
                ),
                Forge::GitHub | Forge::GitLab => events::GITHUB_PAGED,
            };
            let asking = events::Asking {
                forge: self.forge,
                paged,
                api: &self.api,
                name: &self.owner,
                owner,
                token: self.token.as_ref(),
                parse: self.reading().parse,
                tags: self.options.may_take(herald_core::Signal::Tag),
            };
            Some(events::changed(&Erased(http), &asking, read).await)
        })
    }

    fn batch(&self) -> NonZeroUsize {
        self.graphql
            .as_ref()
            .map_or(NonZeroUsize::MIN, |_| self.options.graphql_batch())
    }

    fn rounded<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        owner: Owner,
    ) -> Boxed<'a, Result<Rounded, Unpolled>> {
        Box::pin(async move {
            if self.forge == Forge::GitLab || !self.options.may_take(Signal::Package) {
                return Ok(Rounded::Nothing);
            }
            let (origin, list) = (
                Origin::new(self.forge, &self.options, &self.owner),
                PackageList::Own,
            );
            let packages = self
                .asking(&origin, &list, self.options.prerelease.unwrap_or_default())
                .listed(&Erased(http), (&self.owner, owner), None)
                .await?;
            Ok(Rounded::Read {
                named: packages.linked().cloned().collect(),
                read: Arc::new(packages),
            })
        })
    }

    fn polled<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        members: &'a [(&'a Listed, Option<&'a Cursor>)],
        rounded: &'a Rounded,
        now: jiff::Timestamp,
    ) -> Boxed<'a, Vec<Result<Option<Polled>, Unpolled>>> {
        Box::pin(async move {
            let list = match rounded {
                Rounded::Nothing => PackageList::Own,
                Rounded::Read { read, .. } => Arc::clone(read)
                    .downcast::<OwnerPackages>()
                    .map_or(PackageList::Kept, PackageList::Owners),
                Rounded::Unread => PackageList::Kept,
            };
            let Some(endpoint) = &self.graphql else {
                let mut answered = Vec::new();
                for (repository, cursor) in members {
                    let list = match &list {
                        PackageList::Owners(owned)
                            if !owned.holds(&repository.id) && seen_packages(*cursor) =>
                        {
                            PackageList::Kept
                        }
                        list => list.clone(),
                    };
                    answered.push(
                        self.releases(repository, list)
                            .polled(http, *cursor, now)
                            .await,
                    );
                }
                return answered;
            };
            let asking = Asking {
                forge: self.forge,
                endpoint,
                token: self.token.as_ref(),
                page: self.reading().page,
                parse: self.reading().parse,
                watched: self.watched,
            };
            let signalled: Vec<(&Listed, Option<&Cursor>, crate::Signals)> = members
                .iter()
                .map(|(repository, cursor)| {
                    (
                        *repository,
                        *cursor,
                        self.options.signals_of(&repository.name),
                    )
                })
                .collect();
            let http = Erased(http);
            let answered = polled_together(&http, &asking, &signalled).await;
            let mut polled = Vec::new();
            for (answered, (repository, cursor, signals)) in answered.into_iter().zip(&signalled) {
                let answered = match answered {
                    Ok(found) if signals.takes(Signal::Package) => self
                        .packaged(&http, (repository, &list), (signals, *cursor))
                        .await
                        .map(|packaged| merged(found, packaged, *cursor)),
                    answered => answered,
                };
                let answered = match answered {
                    Ok(found) if signals.takes(Signal::Run) => self
                        .ran(&http, repository, *cursor)
                        .await
                        .map(|(asked, runs)| {
                            crate::runs::merged_into(found, (&asked, runs), *cursor)
                        }),
                    answered => answered,
                };
                polled.push(answered.and_then(|found| {
                    found
                        .map(|found| projected(found, self.named_project(repository)))
                        .transpose()
                }));
            }
            polled
        })
    }

    fn member(&self, repository: &Listed) -> Arc<dyn Poll> {
        Arc::new(self.releases(repository, PackageList::Own))
    }

    fn tag_window(&self, repository: &Listed) -> Option<SignedDuration> {
        self.options
            .held_for(&self.options.signals_of(&repository.name))
    }

    fn origin(&self, repository: &Listed) -> herald_core::Origin {
        herald_core::Origin::of_member(
            self.forge.kind(),
            &crate::forge::host_of(self.forge, &self.api),
            repository,
        )
    }

    fn reach(&self, repository: &Listed) -> herald_core::Reach {
        self.options.reach_of(&repository.name)
    }

    fn project(&self, repository: &Listed) -> Result<String, String> {
        self.template_for(repository).map_or_else(
            || Ok(repository.name.clone()),
            |project| {
                project
                    .render(repository)
                    .map_err(|error| error.to_string())
            },
        )
    }
}

/// The visibility a forge names, as GitHub's and GitLab's listings spell
/// it; any other name is private, so an unknown one is never announced to
/// a public place.
pub(crate) fn named(name: &str) -> Visibility {
    match name {
        "public" => Visibility::Public,
        "internal" => Visibility::Internal,
        _ => Visibility::Private,
    }
}

/// The visibility a repository's flags give: private unless it says it is
/// not, and internal where it says so.
pub(crate) const fn flagged(private: Option<bool>, internal: Option<bool>) -> Visibility {
    match (private, internal) {
        (Some(false), Some(true)) => Visibility::Internal,
        (Some(false), _) => Visibility::Public,
        _ => Visibility::Private,
    }
}

/// The visibility GitLab's numeric level gives: 20 public, 10 internal,
/// and private otherwise.
pub(crate) const fn levelled(level: Option<u8>) -> Visibility {
    match level {
        Some(20) => Visibility::Public,
        Some(10) => Visibility::Internal,
        _ => Visibility::Private,
    }
}

/// A custom property's value, as GitHub names it.
#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum Property {
    One(String),
    Many(Vec<String>),
}

/// Each custom property `properties` sets, with its values; one GitHub
/// leaves unset is left out.
pub(crate) fn properties(
    properties: BTreeMap<String, Option<Property>>,
) -> BTreeMap<String, Vec<String>> {
    properties
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
        .collect()
}

/// A repository, as GitHub lists it.
#[derive(Deserialize)]
pub(crate) struct GitHub {
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
            properties: properties(repository.custom_properties),
            visibility,
            archived: repository.archived,
            fork: repository.fork,
            template: repository.is_template,
            language: repository.language,
            linked: None,
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
            linked: None,
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
            linked: None,
        }
    }
}

/// The GraphQL endpoint beside `api`, the REST API of `forge`.
fn graphql_beside(forge: Forge, api: &str) -> String {
    if forge == Forge::GitLab {
        crate::gitlab::graphql(api)
    } else {
        herald_wire::github::graphql(api)
    }
}

#[cfg(test)]
mod tests;
