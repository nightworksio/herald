//! A repository's package versions in its forge's registry, each announced
//! as the `package` signal: GitHub and Forgejo list an owner's packages and
//! name the repository each is linked to, and GitLab lists a project's
//! (ADR-0035).

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::Arc;

use herald_core::{
    Address, Attribute, Cursor, Draft, Event, Glob, Identity, Kind, Listed, Owner, PACKAGE,
    PACKAGE_TYPE, Polled, PrereleaseRule, RELEASE_LINK, SIGNAL, Signal, Validators, Visibility,
    versioned,
};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Http as _, Request, Response};
use herald_wire::SourceError;
use jiff::Timestamp;
use serde::Deserialize;

use crate::forge::{Repository, api_of};
use crate::options::{PackageVisibility, SourceOptions};
use crate::{Forge, Origin, Token};

mod gitlab;
mod owners;

pub(crate) use owners::OwnerPackages;

/// How GitHub names an organization as a repository's owner.
const ORGANIZATION: &str = "Organization";
/// The type of package a container image is.
const CONTAINER: &str = "container";

/// What a source announces of a repository's packages: their types and
/// names, the container tags never announced, the registry its images are
/// named on, and how much one listing reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Packaging {
    types: Option<Vec<String>>,
    packages: Vec<Glob>,
    moving: Vec<Glob>,
    registry: Option<String>,
    most: NonZeroUsize,
    listed_page: NonZeroUsize,
    seen: PackageVisibility,
}

impl Packaging {
    /// What `options` say of the packages of a repository on `forge`.
    pub(crate) fn of(forge: Forge, options: &SourceOptions) -> Self {
        let registry = options.registry.clone().or_else(|| {
            (forge == Forge::GitHub)
                .then(|| herald_wire::github::containers(&api_of(forge, options)))
        });
        Self {
            types: options.package_types.clone(),
            packages: options.packages().unwrap_or_default(),
            moving: options.moving().unwrap_or_default(),
            registry,
            most: options.most_packages(),
            listed_page: options.listed_page.unwrap_or(forge.api().listed_page),
            seen: options.package_visibility.unwrap_or_default(),
        }
    }

    /// Whether it announces packages of the type `kind`.
    fn takes_type(&self, kind: &str) -> bool {
        self.types
            .as_ref()
            .is_none_or(|types| types.iter().any(|taken| taken == kind))
    }

    /// Whether it announces the package `name`.
    fn takes(&self, name: &str) -> bool {
        self.packages.is_empty() || self.packages.iter().any(|glob| glob.matches(name))
    }

    /// Whether the container tag `tag` moves from one image to the next.
    fn moves(&self, tag: &str) -> bool {
        self.moving.iter().any(|glob| glob.matches(tag))
    }

    /// Who may see a version of a package seen as `package` of a repository
    /// seen as `repository`.
    fn seen(&self, package: Visibility, repository: Visibility) -> Visibility {
        match self.seen {
            PackageVisibility::Stricter => package.max(repository),
            PackageVisibility::Package => package,
        }
    }

    /// The one type a listing may ask the forge for alone, where it
    /// announces only one.
    fn only_type(&self) -> Option<&str> {
        match self.types.as_deref() {
            Some([kind]) => Some(kind),
            _ => None,
        }
    }
}

/// Where a repository's packages are listed from: its own owner's listing,
/// asked by its own poll; the listing its owner source read once for the
/// round; or nowhere this round.
#[derive(Debug, Clone, Default)]
pub(crate) enum PackageList {
    /// Its own poll lists them.
    #[default]
    Own,
    /// Its owner source listed them once for the round.
    Owners(Arc<OwnerPackages>),
    /// They are not asked this round, and are left as its cursor has them:
    /// its owner source's listing failed, or names none of its.
    Kept,
}

/// How a repository's packages are asked for: where, with what token, what
/// the source announces of them, how it tells a pre-release, where they
/// are listed from, and who may see the repository, where its owner's
/// listing already said.
pub(crate) struct Asking<'a> {
    pub(crate) origin: &'a Origin,
    pub(crate) token: Option<&'a Token>,
    pub(crate) packaging: &'a Packaging,
    pub(crate) rule: PrereleaseRule,
    pub(crate) list: &'a PackageList,
    pub(crate) repository: Option<Visibility>,
}

/// What one poll found of a repository's packages: the events of its
/// newest versions, the versions it remembers without reading them, the
/// validators of each package's versions, whether the cursor after it
/// marks packages polled, and whether anything changed since the cursor.
#[derive(Debug, Default)]
pub(crate) struct Packaged {
    pub(crate) events: Vec<Event>,
    pub(crate) unread: Vec<Identity>,
    pub(crate) validators: BTreeMap<String, Validators>,
    pub(crate) marked: bool,
    pub(crate) changed: bool,
}

impl Packaged {
    /// What a poll that asked nothing of the packages keeps of `cursor`.
    pub(crate) fn kept(cursor: Option<&Cursor>) -> Self {
        Self {
            validators: cursor
                .map(|cursor| cursor.packages.clone())
                .unwrap_or_default(),
            marked: seen_packages(cursor),
            ..Self::default()
        }
    }
}

/// One version of a package, as a forge lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    identity: Identity,
    package: String,
    kind: String,
    version: String,
    url: Option<String>,
    published: Timestamp,
    visibility: Visibility,
}

/// The repository's owner, as its forge describes the repository.
#[derive(Deserialize)]
struct Described {
    owner: Option<Holder>,
    path_with_namespace: Option<String>,
    visibility: Option<String>,
    private: Option<bool>,
    internal: Option<bool>,
}

impl Described {
    /// Who may see the repository, as `forge` describes it: private where
    /// it does not say.
    fn visibility(&self, forge: Forge) -> Visibility {
        match (forge, self.visibility.as_deref()) {
            (Forge::GitHub | Forge::GitLab, Some(name)) => crate::owner::named(name),
            (Forge::GitLab, None) => Visibility::Private,
            (Forge::GitHub | Forge::Forgejo, _) => {
                crate::owner::flagged(self.private, self.internal)
            }
        }
    }
}

#[derive(Deserialize)]
struct Holder {
    login: String,
    #[serde(rename = "type")]
    kind: Option<String>,
}

/// The repository a package is linked to.
#[derive(Deserialize)]
struct Linked {
    id: u64,
}

/// `response` from `origin` read as JSON as `what`, within the bytes it
/// parses.
fn read<T: serde::de::DeserializeOwned>(
    origin: &Origin,
    what: &'static str,
    response: &Response,
) -> Result<T, Unpolled> {
    crate::limited::checked(origin.forge(), what, response)?;
    herald_wire::read(what, origin.reading().parse, &response.body).map_err(Unpolled::unreadable)
}

/// The identity of the image `image` of `owner`, tagged `tag`, on
/// `registry`: one a registry's own source of the image shares.
pub(crate) fn container(registry: &str, (owner, image): (&str, &str), tag: &str) -> Identity {
    Identity::new(format!(
        "{}/{}/{}/{tag}",
        registry.to_ascii_lowercase(),
        owner.to_ascii_lowercase(),
        image.to_ascii_lowercase()
    ))
}

/// The identity of the version `version` of the package `package` of the
/// type `kind`, of `owner` on the forge at `host`.
fn packaged(host: &str, (kind, owner, package): (&str, &str, &str), version: &str) -> Identity {
    Identity::new(format!("{host}/{kind}/{owner}/{package}/{version}"))
}

/// The host of `page`, a web page `what` names.
fn host_of(page: Option<&str>, what: &str) -> Result<String, Unpolled> {
    page.and_then(Address::parse)
        .map(|address| address.host().to_owned())
        .ok_or_else(|| Unpolled::Unreadable(format!("{what} names no web page")))
}

/// Whether `cursor` has polled packages before.
pub(crate) fn seen_packages(cursor: Option<&Cursor>) -> bool {
    cursor.is_some_and(|cursor| cursor.validators(Signal::Package).is_some())
}

impl Asking<'_> {
    async fn ask(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        request: Request,
    ) -> Result<Response, Unpolled> {
        http.send(request).await.map_err(Unpolled::unreachable)
    }

    /// Why a listing of more than the source reads is refused.
    fn over(&self, what: &'static str) -> Unpolled {
        Unpolled::Unreadable(format!(
            "{what} list more than {} entries, most_packages; raise it, or narrow package_types",
            self.packaging.most
        ))
    }

    /// The event of `found`, of a repository announced as `project` and
    /// seen as `repository`, its pre-release told by the source's rule.
    fn event(
        &self,
        what: &'static str,
        (project, repository): (&str, Visibility),
        found: Found,
    ) -> Result<Event, Unpolled> {
        let visibility = self.packaging.seen(found.visibility, repository);
        let mut attributes =
            versioned(found.version.clone(), self.rule.marks(None, &found.version));
        for (name, value) in [
            (SIGNAL, Signal::Package.name().to_owned()),
            (PACKAGE, found.package.clone()),
            (PACKAGE_TYPE, found.kind),
        ] {
            attributes.insert(name.to_owned(), Attribute::Text(value));
        }
        let draft = Draft {
            identity: found.identity.as_str().to_owned(),
            kind: Kind::PACKAGE_VERSION,
            project: project.to_owned(),
            attributes,
            title: format!("{} {}", found.package, found.version),
            body: String::new(),
            links: found
                .url
                .map(|url| BTreeMap::from([(RELEASE_LINK.to_owned(), url)]))
                .unwrap_or_default(),
            occurred: found.published,
        };
        Event::normalise(draft)
            .map(|event| event.seen_as(visibility))
            .map_err(|problem| Unpolled::unreadable(SourceError::Invalid { what, problem }))
    }

    /// `found`, the versions of a repository announced as `project` and
    /// seen as `repository`, made what a poll found since `cursor`: the
    /// newest page of them as events where `cursor` has polled packages
    /// before or the source is new, and every other remembered unannounced.
    fn finished(
        &self,
        (project, repository, mut versions): (&str, Visibility, Vec<Found>),
        cursor: Option<&Cursor>,
        mut found: Packaged,
    ) -> Result<Packaged, Unpolled> {
        versions.sort_by_key(|version| std::cmp::Reverse(version.published));
        let page = self.origin.reading().page;
        let what = self.origin.forge().api().versions;
        let fresh = cursor.is_none() || seen_packages(cursor);
        for (at, version) in versions.into_iter().enumerate() {
            if fresh && at < page {
                found
                    .events
                    .push(self.event(what, (project, repository), version)?);
            } else {
                found.unread.push(version.identity);
            }
        }
        found.marked = true;
        let changed = cursor.is_none_or(|cursor| {
            cursor.validators(Signal::Package).is_none()
                || found.events.iter().any(|event| cursor.unseen(event))
                || found
                    .unread
                    .iter()
                    .any(|identity| !cursor.seen.contains(identity))
                || cursor.packages != found.validators
        });
        found.changed = changed;
        Ok(found)
    }
}

/// The package versions of `repository`, which the forge describes in
/// `described`, asked through `http` as `asking` says, since `cursor`.
pub(crate) async fn packaged_of(
    http: &Erased<'_, dyn DynHttp + '_>,
    asking: &Asking<'_>,
    (repository, described): (&Repository, &Response),
    cursor: Option<&Cursor>,
) -> Result<Packaged, Unpolled> {
    if matches!(asking.list, PackageList::Kept) {
        return Ok(Packaged::kept(cursor));
    }
    let forge = asking.origin.forge();
    let what = forge.api().repository;
    let named: Described =
        crate::wire::read(what, &described.body).map_err(Unpolled::unreadable)?;
    let mut found = Packaged::default();
    let versions = if forge == Forge::GitLab {
        let path = named
            .path_with_namespace
            .as_deref()
            .ok_or_else(|| Unpolled::Unreadable(format!("{what} names no full path")))?;
        let visibility = named.visibility(forge);
        asking
            .gitlab(http, (repository, path, visibility), cursor, &mut found)
            .await?
    } else {
        let id = repository.id.to_string();
        let owned = if let PackageList::Owners(owned) = asking.list {
            Arc::clone(owned)
        } else {
            let holder = named
                .owner
                .as_ref()
                .ok_or_else(|| Unpolled::Unreadable(format!("{what} names no owner")))?;
            let owner = if holder.kind.as_deref() == Some(ORGANIZATION) {
                Owner::Organization
            } else {
                Owner::User
            };
            Arc::new(
                asking
                    .listed(http, (&holder.login, owner), Some(&id))
                    .await?,
            )
        };
        asking
            .versions(http, (&owned, &id), cursor, &mut found)
            .await?
    };
    let seen = asking.repository.unwrap_or_else(|| named.visibility(forge));
    asking.finished((&repository.name, seen, versions), cursor, found)
}

/// The package versions of `member`, an owner's member, from `owned`, the
/// listing its owner source read once for the round, asked through `http`
/// as `asking` says, since `cursor`.
pub(crate) async fn packaged_member(
    http: &Erased<'_, dyn DynHttp + '_>,
    asking: &Asking<'_>,
    (member, owned): (&Listed, &OwnerPackages),
    cursor: Option<&Cursor>,
) -> Result<Packaged, Unpolled> {
    let mut found = Packaged::default();
    let versions = asking
        .versions(http, (owned, &member.id), cursor, &mut found)
        .await?;
    asking.finished((&member.name, member.visibility, versions), cursor, found)
}

/// `polled`, a poll's answer where it found anything, with `packaged`, what
/// the same poll found of the packages since `cursor`: nothing where
/// neither changed.
pub(crate) fn merged(
    polled: Option<Polled>,
    packaged: Packaged,
    cursor: Option<&Cursor>,
) -> Option<Polled> {
    if polled.is_none() && !packaged.changed {
        return None;
    }
    let mut polled = polled.unwrap_or_else(|| Polled {
        etag: cursor.and_then(|cursor| cursor.etag.clone()),
        last_modified: cursor.and_then(|cursor| cursor.last_modified.clone()),
        signals: cursor
            .map(|cursor| cursor.signals.clone())
            .unwrap_or_default(),
        ..Polled::default()
    });
    polled.events.extend(packaged.events);
    polled
        .events
        .sort_by_key(|event| std::cmp::Reverse(event.occurred()));
    polled.unread = packaged.unread;
    polled.packages = packaged.validators;
    if packaged.marked {
        polled
            .signals
            .insert(Signal::Package, Validators::default());
    }
    Some(polled)
}

#[cfg(test)]
mod tests;
