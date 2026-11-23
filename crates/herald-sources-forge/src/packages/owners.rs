//! An owner's packages on GitHub and Forgejo, listed once and each kept by
//! the repository it is linked to (ADR-0035).

use std::collections::BTreeMap;

use herald_core::{Cursor, Owner, Visibility};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased};
use jiff::Timestamp;
use serde::Deserialize;

use super::{Asking, CONTAINER, Found, Linked, Packaged, container, host_of, packaged, read};
use crate::Forge;
use crate::forge::{OWNER, validators, whole};
use crate::options::GITHUB_PACKAGE_TYPES;
use crate::owner::{named, next};

/// What a request names a package.
const PACKAGE_NAME: &str = "package name";

/// How Forgejo names an owner any signed-in user may see.
const LIMITED: &str = "limited";
/// The prefix of a container version Forgejo names by its digest alone.
const DIGEST: &str = "sha256:";

/// An owner's packages, as one listing found them, by the forge id of the
/// repository each is linked to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnerPackages {
    /// Where the owner's packages are asked: `/orgs/<owner>/packages` or
    /// `/users/<owner>/packages` on GitHub, and `/packages/<owner>` on
    /// Forgejo.
    base: String,
    /// The owner, as the forge names it.
    login: String,
    linked: BTreeMap<String, Vec<Entry>>,
}

impl OwnerPackages {
    /// The forge id of each repository a package is linked to.
    pub(crate) fn linked(&self) -> impl Iterator<Item = &String> {
        self.linked.keys()
    }

    /// Whether a package is linked to the repository `id`.
    pub(crate) fn holds(&self, id: &str) -> bool {
        self.linked.contains_key(id)
    }
}

/// A package an owner's listing names: a GitHub package, whose versions are
/// asked apart, or a Forgejo package version.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    /// A GitHub package of the type `kind`, on the forge at `host`, seen as
    /// `visibility` lets it be.
    Package {
        kind: &'static str,
        name: String,
        host: String,
        visibility: Visibility,
    },
    /// A Forgejo package version.
    Version(Found),
}

/// A package, as GitHub lists it.
#[derive(Deserialize)]
struct GitHubPackage {
    name: String,
    html_url: String,
    repository: Option<Linked>,
    visibility: Option<String>,
}

/// A package version, as GitHub lists it.
#[derive(Deserialize)]
struct GitHubVersion {
    name: String,
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

/// A package version, as Forgejo lists an owner's.
#[derive(Deserialize)]
struct ForgejoPackage {
    owner: Option<ForgejoOwner>,
    repository: Option<Linked>,
    #[serde(rename = "type")]
    kind: String,
    name: String,
    version: String,
    created_at: Timestamp,
    html_url: String,
}

/// A package's owner, as Forgejo names it, by who may see it.
#[derive(Deserialize)]
struct ForgejoOwner {
    visibility: Option<String>,
}

/// Who may see a package GitHub names `visibility`: private where it names
/// none, or one herald does not know.
fn github_visibility(visibility: Option<&str>) -> Visibility {
    visibility.map_or(Visibility::Private, named)
}

/// Who may see a package Forgejo lists of `owner`, which a package takes
/// from its owner: any signed-in user of a `limited` owner's, as internal,
/// and private where Forgejo names none, or one herald does not know.
fn forgejo_visibility(owner: Option<&ForgejoOwner>) -> Visibility {
    match owner.and_then(|owner| owner.visibility.as_deref()) {
        Some(LIMITED) => Visibility::Internal,
        visibility => github_visibility(visibility),
    }
}

/// The forge id of the repository `linked` names, where it names one and
/// it is `only`, where that is given.
fn kept(linked: Option<&Linked>, only: Option<&str>) -> Option<String> {
    linked
        .map(|linked| linked.id.to_string())
        .filter(|id| only.is_none_or(|only| only == id))
}

impl Asking<'_> {
    /// The packages the forge lists for `login`, an `account`, each kept by
    /// the repository it is linked to where the source takes it; of the
    /// repository `only` alone where it is given.
    pub(crate) async fn listed(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (login, account): (&str, Owner),
        only: Option<&str>,
    ) -> Result<OwnerPackages, Unpolled> {
        let name = whole(login, OWNER)?;
        let mut owned = OwnerPackages {
            base: match (self.origin.forge(), account) {
                (Forge::Forgejo, _) => format!("/packages/{name}"),
                (Forge::GitHub | Forge::GitLab, Owner::Organization) => {
                    format!("/orgs/{name}/packages")
                }
                (Forge::GitHub | Forge::GitLab, Owner::User) => format!("/users/{name}/packages"),
            },
            login: login.to_owned(),
            linked: BTreeMap::new(),
        };
        if self.origin.forge() == Forge::Forgejo {
            self.forgejo(http, &mut owned, only).await?;
        } else {
            self.github(http, &mut owned, only).await?;
        }
        Ok(owned)
    }

    /// Each GitHub package of `owned`'s owner of a type the source takes,
    /// kept in `owned` by the repository it is linked to, `only` where it
    /// is given.
    async fn github(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        owned: &mut OwnerPackages,
        only: Option<&str>,
    ) -> Result<(), Unpolled> {
        let what = Forge::GitHub.api().packages;
        let mut listed = 0;
        for kind in GITHUB_PACKAGE_TYPES
            .into_iter()
            .filter(|kind| self.packaging.takes_type(kind))
        {
            let mut number = 1;
            loop {
                let path = format!(
                    "{}?package_type={kind}&per_page={}&page={number}",
                    owned.base, self.packaging.listed_page
                );
                let answer = self
                    .ask(http, self.origin.rooted(&path, self.token))
                    .await?;
                let page: Vec<GitHubPackage> = read(self.origin, what, &answer)?;
                listed += page.len();
                if listed > self.packaging.most.get() {
                    return Err(self.over(what));
                }
                let more = next(&answer) && !page.is_empty();
                for package in page {
                    let Some(id) = kept(package.repository.as_ref(), only)
                        .filter(|_| self.packaging.takes(&package.name))
                    else {
                        continue;
                    };
                    let host = host_of(Some(&package.html_url), what)?;
                    owned.linked.entry(id).or_default().push(Entry::Package {
                        kind,
                        name: package.name,
                        host,
                        visibility: github_visibility(package.visibility.as_deref()),
                    });
                }
                if !more {
                    break;
                }
                number += 1;
            }
        }
        Ok(())
    }

    /// Each Forgejo package version of `owned`'s owner the source takes,
    /// kept in `owned` by the repository it is linked to: newest first
    /// until the repository `only` has a page of them, where it is given,
    /// and otherwise every one.
    async fn forgejo(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        owned: &mut OwnerPackages,
        only: Option<&str>,
    ) -> Result<(), Unpolled> {
        let what = Forge::Forgejo.api().packages;
        let only_type = self
            .packaging
            .only_type()
            .map_or_else(String::new, |kind| format!("&type={kind}"));
        let wanted = self.origin.reading().page;
        let mut listed = 0;
        let mut number = 1;
        loop {
            let path = format!(
                "{}?limit={}&page={number}{only_type}",
                owned.base, self.packaging.listed_page
            );
            let answer = self
                .ask(http, self.origin.rooted(&path, self.token))
                .await?;
            let page: Vec<ForgejoPackage> = read(self.origin, what, &answer)?;
            listed += page.len();
            if listed > self.packaging.most.get() {
                return Err(self.over(what));
            }
            let more = next(&answer) && !page.is_empty();
            for package in page {
                if let Some((id, found)) = self.forgejo_found(&owned.login, package, only)? {
                    owned
                        .linked
                        .entry(id)
                        .or_default()
                        .push(Entry::Version(found));
                }
            }
            let found = only
                .and_then(|only| owned.linked.get(only))
                .map_or(0, Vec::len);
            if !more || found >= wanted {
                return Ok(());
            }
            number += 1;
        }
    }

    /// The forge id of the repository `package`, a version Forgejo lists of
    /// `login`'s, is linked to and the version, where the source takes it
    /// and the repository is `only`, where that is given.
    fn forgejo_found(
        &self,
        login: &str,
        package: ForgejoPackage,
        only: Option<&str>,
    ) -> Result<Option<(String, Found)>, Unpolled> {
        let containing = package.kind == CONTAINER;
        let taken = self.packaging.takes_type(&package.kind)
            && self.packaging.takes(&package.name)
            && !(containing
                && (package.version.starts_with(DIGEST) || self.packaging.moves(&package.version)));
        let Some(id) = kept(package.repository.as_ref(), only).filter(|_| taken) else {
            return Ok(None);
        };
        let host = host_of(Some(&package.html_url), Forge::Forgejo.api().packages)?;
        let visibility = forgejo_visibility(package.owner.as_ref());
        let identity = if containing {
            let registry = self.packaging.registry.as_deref().unwrap_or(&host);
            container(registry, (login, &package.name), &package.version)
        } else {
            packaged(
                &host,
                (&package.kind, login, &package.name),
                &package.version,
            )
        };
        Ok(Some((
            id,
            Found {
                identity,
                package: package.name,
                kind: package.kind,
                version: package.version,
                url: Some(package.html_url),
                published: package.created_at,
                visibility,
            },
        )))
    }

    /// The versions GitHub's `version` of the package `name` of the type
    /// `kind`, `login`'s on the forge at `host`, stands for, each seen as
    /// `visibility` lets it be: itself, or each of a container version's
    /// tags that does not move.
    fn github_found(
        &self,
        (kind, name, host, login): (&'static str, &str, &str, &str),
        (version, visibility): (GitHubVersion, Visibility),
    ) -> Vec<Found> {
        if kind != CONTAINER {
            return vec![Found {
                identity: packaged(host, (kind, login, name), &version.name),
                package: name.to_owned(),
                kind: kind.to_owned(),
                version: version.name,
                url: version.html_url,
                published: version.created_at,
                visibility,
            }];
        }
        let registry = self.packaging.registry.as_deref().unwrap_or(host);
        version
            .metadata
            .and_then(|metadata| metadata.container)
            .map(|container| container.tags)
            .unwrap_or_default()
            .into_iter()
            .filter(|tag| !self.packaging.moves(tag))
            .map(|tag| Found {
                identity: container(registry, (login, name), &tag),
                package: name.to_owned(),
                kind: CONTAINER.to_owned(),
                version: tag,
                url: version.html_url.clone(),
                published: version.created_at,
                visibility,
            })
            .collect()
    }

    /// The versions of the packages `owned` keeps for the repository `id`:
    /// each GitHub package's asked conditionally on the validators `cursor`
    /// keeps for it, which `found` records, and each Forgejo version as
    /// listed.
    pub(super) async fn versions(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (owned, id): (&OwnerPackages, &str),
        cursor: Option<&Cursor>,
        found: &mut Packaged,
    ) -> Result<Vec<Found>, Unpolled> {
        let what = Forge::GitHub.api().versions;
        let mut versions = Vec::new();
        for entry in owned.linked.get(id).into_iter().flatten() {
            let (kind, name, host, visibility) = match entry {
                Entry::Version(listed) => {
                    versions.push(listed.clone());
                    continue;
                }
                Entry::Package {
                    kind,
                    name,
                    host,
                    visibility,
                } => (*kind, name, host, *visibility),
            };
            let key = format!("{kind}/{name}");
            let path = format!(
                "{}/{kind}/{}/versions?per_page={}",
                owned.base,
                whole(name, PACKAGE_NAME)?,
                self.origin.reading().page
            );
            let held = cursor.and_then(|cursor| cursor.packages.get(&key));
            let request = self.origin.rooted(&path, self.token).conditional(
                held.and_then(|held| held.etag.as_deref()),
                held.and_then(|held| held.last_modified.as_deref()),
            );
            let answer = self.ask(http, request).await?;
            if let Some(held) = held.filter(|_| Forge::unchanged(&answer)) {
                found.validators.insert(key, held.clone());
                continue;
            }
            let listed: Vec<GitHubVersion> = read(self.origin, what, &answer)?;
            found.validators.insert(key, validators(&answer));
            versions.extend(listed.into_iter().flat_map(|version| {
                self.github_found((kind, name, host, &owned.login), (version, visibility))
            }));
        }
        Ok(versions)
    }
}
