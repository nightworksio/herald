//! A GitHub owner's container images on its registry, each polled on its
//! own and announced as the project of the repository it is linked to, or
//! as its own name where it is linked to none (ADR-0040).

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, PoisonError};

use herald_core::{
    Cursor, Draft, Duration, Event, Glob, KEEP_MEMBERS, Kind, Listed, Listing, Owner, PACKAGE,
    PACKAGE_TYPE, Polled, ProjectName, RELEASE_LINK, Sandbox, SecretRef, Selection, Size,
    Visibility, path_of_names, prerelease_version, status, versioned, web_url,
};
use herald_kinds::{
    Credential, Expand, Expanded, Fault, Poll, Polling, SourceConfig, SourceKind, Unlisted,
    Unpolled,
};
use herald_ports::{Boxed, DynHttp, Erased, Http as _, Method, Request, Response, SecretText};
use herald_wire::{PARSE_LIMIT, Reading, SourceError, answered};
use jiff::{SignedDuration, Timestamp};
use serde::Deserialize;

use crate::forge::{OWNER, authorized, whole};
use crate::options::{
    MOST_PACKAGES, MOST_PACKAGES_ALLOWED, MOVING, PackageVisibility, SourceOptions, registry_host,
};
use crate::owner::{GitHub, named, next};
use crate::packages::container;
use crate::{Forge, Token};

/// A `ghcr` source's options in `herald.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GhcrOptions {
    /// The GitHub organization or user whose container images the source
    /// takes.
    pub owner: String,
    /// A token that may read the owner's packages, which GitHub asks for
    /// even for public images.
    pub token: SecretRef,
    /// GitHub's REST API, for a GitHub Enterprise Server or a GHE.com
    /// tenant; `https://api.github.com` where absent.
    pub api: Option<String>,
    /// The registry's host the images are named on, in their identities;
    /// ghcr.io beside api.github.com, and `containers.<host>` beside a
    /// GitHub Enterprise Server or a GHE.com tenant, where absent.
    pub registry: Option<String>,
    /// Globs of the image names taken; every name where empty.
    #[serde(default)]
    pub include: Vec<String>,
    /// Globs of the image names left out, whatever `include` says.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// The visibilities of the images taken; `["public"]` where absent.
    pub visibility: Option<Vec<Visibility>>,
    /// Globs of the tags never announced, as they move from one image to
    /// the next; `["latest"]` where absent.
    pub moving: Option<Vec<String>>,
    /// Who may see an image linked to a repository: `stricter`, the
    /// stricter of the image's visibility and its repository's, where
    /// absent, or `package`, the image's own.
    pub package_visibility: Option<PackageVisibility>,
    /// The most images a listing reads before it refuses the round; 1000
    /// where absent, and at most 100000.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub most_packages: Option<NonZeroUsize>,
    /// How long an image that left the owner is kept, with its cursor;
    /// "30d" where absent.
    pub keep_members: Option<Duration>,
    /// The project an image linked to a repository is announced as: a
    /// template over the repository's `repository.*`, as an owner source
    /// renders it; the repository's name where absent. An image linked to
    /// none is announced as its own name.
    pub project: Option<String>,
    /// How many of the newest versions the first poll announces; none
    /// where absent.
    #[serde(default)]
    pub backfill: usize,
    /// How many of an image's newest versions one poll reads; 30 where
    /// absent.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub page: Option<NonZeroUsize>,
    /// How many identities each image's cursor remembers; 500 where
    /// absent, and at least `page`.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub remembered: Option<NonZeroUsize>,
    /// The most of an answer parsed; "4MiB" where absent, and at most four
    /// times that.
    pub parse: Option<Size>,
}

impl GhcrOptions {
    /// Whether the options are sound.
    ///
    /// # Errors
    ///
    /// The first key that is not.
    pub fn check(&self) -> Result<(), String> {
        if !path_of_names(&self.owner, 1..=1) {
            return Err(format!(
                "\"{}\" is not an owner; give the organization's or user's name, of letters, digits, '.', '_' and '-'",
                self.owner
            ));
        }
        if let Some(api) = self.api.as_ref().filter(|api| !web_url(api)) {
            return Err(format!(
                "\"{api}\" is not a web address; give GitHub's REST API, such as https://ghe.example/api/v3"
            ));
        }
        if let Some(registry) = self
            .registry
            .as_ref()
            .filter(|registry| !registry_host(registry))
        {
            return Err(format!(
                "registry \"{registry}\" is not a registry's host; give it as registry.example or registry.example:5000"
            ));
        }
        self.selection()?;
        self.moving()?;
        if self.most_packages().get() > MOST_PACKAGES_ALLOWED {
            return Err(format!(
                "most_packages is at most {MOST_PACKAGES_ALLOWED}; lower it"
            ));
        }
        if let Some(project) = &self.project {
            SourceOptions::project_check(project).map_err(|reason| format!("project {reason}"))?;
        }
        self.reading().check(PARSE_LIMIT)
    }

    /// The API base URL, without a trailing `/`.
    #[must_use]
    pub fn api(&self) -> String {
        self.api
            .as_deref()
            .unwrap_or(Forge::GitHub.api().url)
            .trim_end_matches('/')
            .to_owned()
    }

    /// The registry's host the images are named on.
    #[must_use]
    pub fn registry(&self) -> String {
        self.registry
            .clone()
            .unwrap_or_else(|| herald_wire::github::containers(&self.api()))
    }

    /// Which images the source takes.
    ///
    /// # Errors
    ///
    /// When a glob cannot be read.
    pub fn selection(&self) -> Result<Selection, String> {
        Ok(Selection {
            include: globs(&self.include)?,
            exclude: globs(&self.exclude)?,
            visibility: self
                .visibility
                .clone()
                .unwrap_or_else(|| vec![Visibility::Public]),
            ..Selection::default()
        })
    }

    /// The globs of the tags never announced.
    ///
    /// # Errors
    ///
    /// When a glob cannot be read.
    pub fn moving(&self) -> Result<Vec<Glob>, String> {
        self.moving.as_ref().map_or_else(
            || globs(&MOVING.map(ToOwned::to_owned)),
            |moving| globs(moving),
        )
    }

    /// The most images one listing reads.
    #[must_use]
    pub fn most_packages(&self) -> NonZeroUsize {
        self.most_packages.unwrap_or(MOST_PACKAGES)
    }

    /// How a poll of an image reads.
    #[must_use]
    pub fn reading(&self) -> Reading {
        Reading::of(self.page, self.remembered, self.parse, PARSE_LIMIT)
    }
}

fn globs(texts: &[String]) -> Result<Vec<Glob>, String> {
    texts
        .iter()
        .map(|text| Glob::new(text).map_err(|error| error.to_string()))
        .collect()
}

/// GitHub's container registry, as a kind of source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GhcrKind {}

impl SourceKind for GhcrKind {
    const NAME: &str = "ghcr";
    const DESCRIPTION: &str = "A GitHub owner's container images, each announced as the project of the repository it is linked to.";

    type File = GhcrOptions;
    type Options = GhcrSource;

    fn checked(file: GhcrOptions) -> Result<GhcrSource, Fault> {
        file.check()
            .map(|()| GhcrSource { options: file })
            .map_err(Fault::here)
    }
}

/// A `ghcr` source: the options its table reads into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhcrSource {
    /// The options its table reads into.
    pub options: GhcrOptions,
}

impl SourceConfig for GhcrSource {
    fn kind(&self) -> &'static str {
        GhcrKind::NAME
    }

    fn token(&self) -> Option<&SecretRef> {
        Some(&self.options.token)
    }

    fn kinds(&self) -> Vec<Kind> {
        vec![Kind::PACKAGE_VERSION]
    }

    fn backfill(&self) -> usize {
        self.options.backfill
    }

    fn unlisted(&self) -> bool {
        self.options
            .selection()
            .is_ok_and(|selection| selection.takes_unlisted())
    }

    fn poller(&self, token: Option<Credential>, _: bool) -> Polling {
        Polling::Many(Arc::new(Images::new(&self.options, token.map(Token::of))))
    }
}

/// A container image, as GitHub lists an owner's packages.
#[derive(Deserialize)]
struct Image {
    id: u64,
    name: String,
    visibility: Option<String>,
    repository: Option<GitHub>,
}

/// A version of an image, as GitHub lists it.
#[derive(Deserialize)]
struct Version {
    html_url: Option<String>,
    created_at: Timestamp,
    metadata: Option<Metadata>,
}

#[derive(Deserialize)]
struct Metadata {
    container: Option<Tags>,
}

#[derive(Deserialize)]
struct Tags {
    tags: Vec<String>,
}

/// A GitHub owner's container images, each taken by the selection polled
/// on its own.
#[derive(Debug)]
pub(crate) struct Images {
    api: String,
    owner: String,
    registry: String,
    selection: Selection,
    moving: Vec<Glob>,
    project: Option<ProjectName>,
    options: GhcrOptions,
    token: Option<Token>,
    kind: Arc<Mutex<Option<Owner>>>,
}

impl Images {
    pub(crate) fn new(options: &GhcrOptions, token: Option<Token>) -> Self {
        Self {
            api: options.api(),
            owner: options.owner.clone(),
            registry: options.registry(),
            selection: options.selection().unwrap_or_default(),
            moving: options.moving().unwrap_or_default(),
            project: options
                .project
                .as_deref()
                .and_then(|project| ProjectName::compile(project, Sandbox::DEFAULT).ok()),
            options: options.clone(),
            token,
            kind: Arc::new(Mutex::new(None)),
        }
    }

    /// The path under which GitHub serves the packages of the owner, as an
    /// `owner`; or why its name names nothing.
    fn base(&self, owner: Owner) -> Result<String, Unpolled> {
        packages(&self.api, (owner, &self.owner))
    }

    fn get(&self, url: String) -> Request {
        authorized(
            Forge::GitHub,
            Request::new(Method::Get, SecretText::plain(url)).at_most(self.options.reading().parse),
            self.token.as_ref(),
        )
    }

    /// The image `image`, as a member, seen as `seen` says.
    fn member_of(image: Image, seen: PackageVisibility) -> Listed {
        let own = image
            .visibility
            .as_deref()
            .map_or(Visibility::Private, named);
        let linked = image
            .repository
            .map(|repository| Box::new(Listed::from(repository)));
        let visibility = match (seen, &linked) {
            (PackageVisibility::Stricter, Some(repository)) => own.max(repository.visibility),
            _ => own,
        };
        Listed {
            id: image.id.to_string(),
            path: image.name.clone(),
            name: image.name,
            topics: Vec::new(),
            properties: BTreeMap::new(),
            visibility,
            archived: false,
            fork: false,
            template: false,
            language: None,
            linked,
        }
    }

    async fn list(
        &self,
        http: &dyn DynHttp,
        known: Option<&Listing>,
    ) -> Result<Expanded, Unlisted> {
        let http = Erased(http);
        let mut owner = known.map_or(Owner::Organization, |known| known.owner);
        let most = self.options.most_packages();
        let page = Forge::GitHub.api().listed_page;
        let mut listed = Vec::new();
        let mut pages = Vec::new();
        loop {
            let number = pages.len() + 1;
            let url = format!(
                "{}?package_type=container&per_page={page}&page={number}",
                self.base(owner).map_err(Unlisted::Failed)?
            );
            let answer = http
                .send(self.get(url))
                .await
                .map_err(|error| Unlisted::Failed(Unpolled::unreachable(error)))?;
            if answer.status == status::NOT_FOUND && number == 1 && owner == Owner::Organization {
                owner = Owner::User;
                continue;
            }
            answered(Forge::GitHub.api().packages, &answer).map_err(failed)?;
            let images: Vec<Image> = herald_wire::read(
                Forge::GitHub.api().packages,
                self.options.reading().parse,
                &answer.body,
            )
            .map_err(failed)?;
            let more = next(&answer) && !images.is_empty();
            pages.push(answer.header("etag").map(ToOwned::to_owned));
            let seen = self.options.package_visibility.unwrap_or_default();
            listed.extend(images.into_iter().map(|image| Self::member_of(image, seen)));
            if listed.len() > most.get() {
                return Err(Unlisted::Over(format!(
                    "{} lists more than {most} container images, most_packages; raise it, or narrow include and exclude",
                    self.owner
                )));
            }
            if !more {
                break;
            }
        }
        *self.kind.lock().unwrap_or_else(PoisonError::into_inner) = Some(owner);
        Ok(Expanded::Listed {
            members: listed
                .into_iter()
                .filter(|image| self.selection.takes(image))
                .collect(),
            owner,
            pages,
            file: None,
            missing: Vec::new(),
        })
    }
}

fn failed(error: SourceError) -> Unlisted {
    Unlisted::Failed(Unpolled::unreadable(error))
}

impl Expand for Images {
    fn api(&self) -> &str {
        &self.api
    }

    fn member_nouns(&self) -> (&'static str, &'static str) {
        ("container image", "container images")
    }

    fn pushed(&self) -> bool {
        false
    }

    fn standing(&self, image: &Listed) -> Option<herald_kinds::Standing> {
        image
            .linked
            .as_ref()
            .map(|repository| herald_kinds::Standing::new(&self.api, &repository.path, true))
    }

    fn keep_members(&self) -> SignedDuration {
        self.options
            .keep_members
            .map_or(KEEP_MEMBERS, |kept| kept.0)
    }

    fn remembered(&self) -> usize {
        self.options.reading().remembered
    }

    fn listed<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        known: Option<&'a Listing>,
    ) -> Boxed<'a, Result<Expanded, Unlisted>> {
        Box::pin(self.list(http, known))
    }

    fn member(&self, image: &Listed) -> Arc<dyn Poll> {
        let known = *self.kind.lock().unwrap_or_else(PoisonError::into_inner);
        let (first, fallback) = match known {
            Some(owner) => (owner, None),
            None => (Owner::Organization, Some(Owner::User)),
        };
        Arc::new(ImageVersions {
            api: self.api.clone(),
            first,
            fallback,
            image: image.name.clone(),
            project: self.project(image).unwrap_or(image.name.clone()),
            owner: self.owner.clone(),
            registry: self.registry.clone(),
            moving: self.moving.clone(),
            visibility: image.visibility,
            reading: self.options.reading(),
            token: self.token.clone(),
        })
    }

    fn origin(&self, image: &Listed) -> herald_core::Origin {
        let package = format!("{}/{}", self.owner, image.name);
        let linked = image.linked.as_deref();
        herald_core::Origin {
            topics: linked
                .map(|repository| repository.topics.clone())
                .unwrap_or_default(),
            properties: linked
                .map(|repository| repository.properties.clone())
                .unwrap_or_default(),
            visibility: image.visibility,
            ..herald_core::Origin::of_package(GhcrKind::NAME, &self.registry, &self.owner, &package)
        }
    }

    fn project(&self, image: &Listed) -> Result<String, String> {
        match (&image.linked, &self.project) {
            (Some(repository), Some(project)) => project
                .render(repository)
                .map_err(|error| error.to_string()),
            (Some(repository), None) => Ok(repository.name.clone()),
            (None, _) => Ok(image.name.clone()),
        }
    }

    fn takes(&self, image: &Listed) -> bool {
        self.selection.takes(image)
    }

    fn fingerprint(&self) -> String {
        herald_core::digest(&format!(
            "ghcr\n{}\n{}\n{:?}\n{:?}",
            self.api,
            self.owner,
            self.selection,
            self.options.package_visibility.unwrap_or_default()
        ))
    }
}

/// One container image's versions, each tag that does not move announced
/// under the registry's identity.
#[derive(Debug)]
struct ImageVersions {
    api: String,
    first: Owner,
    fallback: Option<Owner>,
    image: String,
    project: String,
    owner: String,
    registry: String,
    moving: Vec<Glob>,
    visibility: Visibility,
    reading: Reading,
    token: Option<Token>,
}

/// What a request names an image.
const IMAGE: &str = "image";

/// The path under which GitHub serves the packages of the owner `name`, as
/// an `owner`; or why the name names nothing.
fn packages(api: &str, (owner, name): (Owner, &str)) -> Result<String, Unpolled> {
    let account = match owner {
        Owner::Organization => "orgs",
        Owner::User => "users",
    };
    Ok(format!("{api}/{account}/{}/packages", whole(name, OWNER)?))
}

impl ImageVersions {
    fn request(&self, owner: Owner, cursor: Option<&Cursor>) -> Result<Request, Unpolled> {
        let base = packages(&self.api, (owner, &self.owner))?;
        Ok(authorized(
            Forge::GitHub,
            Request::new(
                Method::Get,
                SecretText::plain(format!(
                    "{base}/container/{}/versions?per_page={}",
                    whole(&self.image, IMAGE)?,
                    self.reading.page
                )),
            )
            .at_most(self.reading.parse),
            self.token.as_ref(),
        )
        .conditional(
            cursor.and_then(|cursor| cursor.etag.as_deref()),
            cursor.and_then(|cursor| cursor.last_modified.as_deref()),
        ))
    }

    fn events(&self, answer: &Response) -> Result<Polled, SourceError> {
        answered(Forge::GitHub.api().versions, answer)?;
        let versions: Vec<Version> = herald_wire::read(
            Forge::GitHub.api().versions,
            self.reading.parse,
            &answer.body,
        )?;
        let mut found: Vec<(String, Option<String>, Timestamp)> = versions
            .into_iter()
            .flat_map(|version| {
                let tags = version
                    .metadata
                    .and_then(|metadata| metadata.container)
                    .map(|container| container.tags)
                    .unwrap_or_default();
                tags.into_iter()
                    .map(move |tag| (tag, version.html_url.clone(), version.created_at))
            })
            .filter(|(tag, _, _)| !self.moving.iter().any(|glob| glob.matches(tag)))
            .collect();
        found.sort_by_key(|(_, _, created)| std::cmp::Reverse(*created));
        found.truncate(self.reading.page);
        let events = found
            .into_iter()
            .map(|(tag, url, created)| self.event(&tag, url, created))
            .collect::<Result<Vec<Event>, SourceError>>()?;
        Ok(Polled {
            events,
            etag: answer.header("etag").map(ToOwned::to_owned),
            last_modified: answer.header("last-modified").map(ToOwned::to_owned),
            ..Polled::default()
        })
    }

    fn event(
        &self,
        tag: &str,
        url: Option<String>,
        created: Timestamp,
    ) -> Result<Event, SourceError> {
        let mut attributes = versioned(tag.to_owned(), prerelease_version(tag));
        attributes.insert(
            PACKAGE.to_owned(),
            herald_core::Attribute::Text(self.image.clone()),
        );
        attributes.insert(
            PACKAGE_TYPE.to_owned(),
            herald_core::Attribute::Text("container".to_owned()),
        );
        let draft = Draft {
            identity: container(&self.registry, (&self.owner, &self.image), tag)
                .as_str()
                .to_owned(),
            kind: Kind::PACKAGE_VERSION,
            project: self.project.clone(),
            title: format!("{} {tag}", self.image),
            attributes,
            body: String::new(),
            links: url
                .map(|url| BTreeMap::from([(RELEASE_LINK.to_owned(), url)]))
                .unwrap_or_default(),
            occurred: created,
        };
        Event::normalise(draft)
            .map(|event| event.seen_as(self.visibility))
            .map_err(|problem| SourceError::Invalid {
                what: Forge::GitHub.api().versions,
                problem,
            })
    }
}

impl Poll for ImageVersions {
    fn api(&self) -> &str {
        &self.api
    }

    fn remembered(&self) -> usize {
        self.reading.remembered
    }

    fn polled<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        cursor: Option<&'a Cursor>,
        _: jiff::Timestamp,
    ) -> Boxed<'a, Result<Option<Polled>, Unpolled>> {
        Box::pin(async move {
            let http = Erased(http);
            let mut answer = http
                .send(self.request(self.first, cursor)?)
                .await
                .map_err(Unpolled::unreachable)?;
            if let Some(fallback) = self
                .fallback
                .as_ref()
                .filter(|_| answer.status == status::NOT_FOUND)
            {
                answer = http
                    .send(self.request(*fallback, cursor)?)
                    .await
                    .map_err(Unpolled::unreachable)?;
            }
            if Forge::unchanged(&answer) {
                return Ok(None);
            }
            self.events(&answer).map(Some).map_err(Unpolled::unreadable)
        })
    }
}

#[cfg(test)]
mod tests;
