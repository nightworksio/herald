//! A GitLab project's packages and its container images' tags
//! (ADR-0035).

use herald_core::{Address, Cursor, Identity, Visibility};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased};
use jiff::Timestamp;
use serde::Deserialize;

use super::{
    Asking, CONTAINER, Found, Packaged, container, host_of, packaged, read, seen_packages,
};
use crate::Forge;
use crate::forge::{Checked, Repository, validators, whole};
use crate::owner::next;

/// The key of a project's listing of packages among the validators a
/// cursor keeps by package.
const LISTING: &str = "packages";
/// What GitLab's listing of a project's container images is called.
const IMAGES: &str = "GitLab's container images";
/// What GitLab's listing of an image's tags is called.
const TAGS: &str = "GitLab's image tags";
/// What GitLab's answer of one image tag is called.
const TAG: &str = "GitLab's image tag";

/// A package version, as GitLab lists a project's.
#[derive(Deserialize)]
struct GitLabPackage {
    name: String,
    version: String,
    package_type: String,
    created_at: Timestamp,
    #[serde(rename = "_links")]
    links: Option<WebPath>,
}

#[derive(Deserialize)]
struct WebPath {
    web_path: Option<String>,
}

/// A container image, as GitLab lists a project's.
#[derive(Deserialize)]
struct GitLabImage {
    id: u64,
    name: String,
}

/// An image tag, as GitLab lists and describes it.
#[derive(Deserialize)]
struct GitLabTag {
    name: String,
    location: String,
    created_at: Option<Timestamp>,
}

impl Asking<'_> {
    /// The newest versions of the packages and container images GitLab
    /// lists for the project `repository`, at `path` on the forge, each
    /// seen as the project's `visibility` lets it be.
    pub(super) async fn gitlab(
        &self,
        http: &Erased<'_, dyn DynHttp + '_>,
        (repository, path, visibility): (&Repository, &str, Visibility),
        cursor: Option<&Cursor>,
        found: &mut Packaged,
    ) -> Result<Vec<Found>, Unpolled> {
        let api = Forge::GitLab.api();
        let parse = self.origin.reading().parse;
        let page = repository.page.as_deref();
        let host = host_of(page, api.repository)?;
        let owner = path.rsplit_once('/').map_or(path, |(owner, _)| owner);
        let checked = self.origin.checked()?;
        let mut versions = Vec::new();
        if self.packaging.only_type() != Some(CONTAINER) {
            let only = self
                .packaging
                .only_type()
                .map_or_else(String::new, |kind| format!("&package_type={kind}"));
            let held = cursor.and_then(|cursor| cursor.packages.get(LISTING));
            let request = checked
                .get(
                    &format!(
                        "/packages?order_by=created_at&sort=desc&per_page={}{only}",
                        self.origin.reading().page
                    ),
                    self.token,
                )
                .conditional(
                    held.and_then(|held| held.etag.as_deref()),
                    held.and_then(|held| held.last_modified.as_deref()),
                );
            let answer = self.ask(http, request).await?;
            if let Some(held) = held.filter(|_| Forge::unchanged(&answer)) {
                found.validators.insert(LISTING.to_owned(), held.clone());
            } else {
                let listed: Vec<GitLabPackage> = read(api.packages, parse, &answer)?;
                found
                    .validators
                    .insert(LISTING.to_owned(), validators(&answer));
                let origin = page
                    .and_then(Address::parse)
                    .map(|address| address.origin())
                    .unwrap_or_default();
                versions.extend(
                    listed
                        .into_iter()
                        .filter(|package| {
                            self.packaging.takes_type(&package.package_type)
                                && self.packaging.takes(&package.name)
                        })
                        .map(|package| Found {
                            identity: packaged(
                                &host,
                                (&package.package_type, owner, &package.name),
                                &package.version,
                            ),
                            url: package
                                .links
                                .and_then(|links| links.web_path)
                                .map(|path| format!("{origin}{path}")),
                            package: package.name,
                            kind: package.package_type,
                            version: package.version,
                            published: package.created_at,
                            visibility,
                        }),
                );
            }
        }
        if self.packaging.takes_type(CONTAINER) {
            versions.extend(
                self.images(
                    (http, &checked),
                    (repository, path, visibility),
                    cursor,
                    found,
                )
                .await?,
            );
        }
        Ok(versions)
    }

    /// Each entry of the pages GitLab answers for `path` under the project
    /// `checked`, as many as the source reads.
    async fn pages<T: serde::de::DeserializeOwned>(
        &self,
        (http, checked): (&Erased<'_, dyn DynHttp + '_>, &Checked<'_>),
        (what, path): (&'static str, &str),
        listed: &mut usize,
    ) -> Result<Vec<T>, Unpolled> {
        let mut entries = Vec::new();
        let mut number = 1;
        loop {
            let asked = format!(
                "{path}?per_page={}&page={number}",
                self.packaging.listed_page
            );
            let answer = self.ask(http, checked.get(&asked, self.token)).await?;
            let page: Vec<T> = read(what, self.origin.reading().parse, &answer)?;
            *listed += page.len();
            if *listed > self.packaging.most.get() {
                return Err(self.over(what));
            }
            let more = next(&answer) && !page.is_empty();
            entries.extend(page);
            if !more {
                return Ok(entries);
            }
            number += 1;
        }
    }

    /// The tags of the container images GitLab lists for the project
    /// `repository`, asked under its `checked` base, seen as `visibility`
    /// lets them be: each unseen one read
    /// in full where the source has seen packages before, and remembered
    /// unread where it has not.
    async fn images(
        &self,
        (http, checked): (&Erased<'_, dyn DynHttp + '_>, &Checked<'_>),
        (repository, path, visibility): (&Repository, &str, Visibility),
        cursor: Option<&Cursor>,
        found: &mut Packaged,
    ) -> Result<Vec<Found>, Unpolled> {
        let mut listed = 0;
        let images: Vec<GitLabImage> = self
            .pages(
                (http, checked),
                (IMAGES, "/registry/repositories"),
                &mut listed,
            )
            .await?;
        let page = repository.page.as_deref().unwrap_or_default();
        let mut versions = Vec::new();
        for image in images {
            let name = if image.name.is_empty() {
                path.to_owned()
            } else {
                format!("{path}/{}", image.name)
            };
            if !self.packaging.takes(&name) {
                continue;
            }
            let base = format!("/registry/repositories/{}/tags", image.id);
            let tags: Vec<GitLabTag> = self
                .pages((http, checked), (TAGS, &base), &mut listed)
                .await?;
            for tag in tags
                .into_iter()
                .filter(|tag| !self.packaging.moves(&tag.name))
            {
                let identity = self.located(&tag.location, &tag.name);
                if cursor.is_some_and(|cursor| cursor.seen.contains(&identity)) {
                    continue;
                }
                if !seen_packages(cursor) {
                    found.unread.push(identity);
                    continue;
                }
                let tagged = format!("{base}/{}", whole(&tag.name, crate::forge::TAG)?);
                let answer = self.ask(http, checked.get(&tagged, self.token)).await?;
                let read: GitLabTag = read(TAG, self.origin.reading().parse, &answer)?;
                let published = read.created_at.ok_or_else(|| {
                    Unpolled::Unreadable(format!("{TAG} {} has no creation time", tag.name))
                })?;
                versions.push(Found {
                    identity,
                    package: name.clone(),
                    kind: CONTAINER.to_owned(),
                    version: tag.name,
                    url: Some(format!(
                        "{}/container_registry/{}",
                        page.trim_end_matches('/'),
                        image.id
                    )),
                    published,
                    visibility,
                });
            }
        }
        Ok(versions)
    }

    /// The identity of the image tag at `location`, tagged `tag`, on the
    /// source's registry where it names one and the location's otherwise.
    fn located(&self, location: &str, tag: &str) -> Identity {
        let image = location
            .strip_suffix(tag)
            .and_then(|image| image.strip_suffix(':'))
            .unwrap_or(location);
        let (host, image) = image.split_once('/').unwrap_or(("", image));
        let registry = self.packaging.registry.as_deref().unwrap_or(host);
        let (owner, image) = image.rsplit_once('/').unwrap_or(("", image));
        container(registry, (owner, image), tag)
    }
}
