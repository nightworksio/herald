use herald_core::{Visibility, unreserved};
use herald_wire::{SourceError, read};
use jiff::Timestamp;
use serde::Deserialize;

use crate::namespace::Page;
use crate::options::TagOrdering;
use crate::package::{Api, RegistryVersion};

pub(crate) const API: Api = Api {
    url: "https://hub.docker.com/v2",
    limit: herald_wire::PARSE_LIMIT,
    versions: "Docker Hub's tags",
};

/// The most of an exchange's answer read: a token and little else.
pub(crate) const TOKEN_ANSWER: usize = 65_536;

#[derive(Deserialize)]
struct Exchanged {
    access_token: String,
}

/// The access token Docker Hub answered an exchange with, where it
/// answered one.
pub(crate) fn exchanged(response: &herald_ports::Response) -> Option<String> {
    response
        .succeeded()
        .then(|| serde_json::from_slice::<Exchanged>(&response.body).ok())
        .flatten()
        .map(|exchanged| exchanged.access_token)
        .filter(|token| !token.is_empty())
}

/// What Docker Hub's listing of a namespace's repositories is called.
pub(crate) const REPOSITORIES: &str = "Docker Hub's repositories of a namespace";

#[derive(Deserialize)]
struct Repositories {
    results: Vec<Repository>,
    next: Option<String>,
}

#[derive(Deserialize)]
struct Repository {
    name: String,
    namespace: String,
    is_private: bool,
}

/// The registry host Docker Hub's images are named on, which their
/// identities share with any other source of the image (ADR-0035).
pub(crate) const REGISTRY: &str = "docker.io";

/// The namespace of Docker Hub's official images.
pub(crate) const OFFICIAL: &str = "library";

#[derive(Deserialize)]
struct Tags {
    results: Vec<Tag>,
}

#[derive(Deserialize)]
struct Tag {
    name: String,
    #[serde(rename = "tag_last_pushed")]
    pushed: Option<Timestamp>,
    #[serde(rename = "last_updated")]
    updated: Option<Timestamp>,
}

fn split(name: &str) -> (&str, &str) {
    name.split_once('/').unwrap_or((OFFICIAL, name))
}

pub(crate) fn namespace(name: &str) -> &str {
    split(name).0
}

pub(crate) fn project(name: &str) -> &str {
    match split(name) {
        (OFFICIAL, image) => image,
        _ => name,
    }
}

pub(crate) fn path(name: &str, page: usize, ordering: TagOrdering) -> String {
    let (namespace, repository) = split(name);
    format!(
        "/namespaces/{}/repositories/{}/tags?page_size={page}&ordering={}",
        unreserved(namespace),
        unreserved(repository),
        ordering.as_str()
    )
}

pub(crate) fn page(name: &str, tag: &str) -> String {
    match split(name) {
        (OFFICIAL, image) => format!("https://hub.docker.com/_/{image}/tags?name={tag}"),
        _ => format!("https://hub.docker.com/r/{name}/tags?name={tag}"),
    }
}

/// Each tag with the time it was last pushed.
pub(crate) fn found(
    api: &Api,
    limit: usize,
    body: &[u8],
) -> Result<Vec<RegistryVersion>, SourceError> {
    let tags: Tags = read(api.versions, limit, body)?;
    Ok(tags
        .results
        .into_iter()
        .filter_map(|tag| {
            tag.pushed.or(tag.updated).map(|published| RegistryVersion {
                version: tag.name,
                published,
            })
        })
        .collect())
}

/// The path of page `number` of `namespace`'s repositories, `page` to a
/// page.
pub(crate) fn repositories_path(namespace: &str, page: usize, number: usize) -> String {
    format!(
        "/namespaces/{}/repositories?page_size={page}&page={number}",
        unreserved(namespace)
    )
}

/// The repositories a page of `namespace`'s listing names, each with who
/// may see it.
///
/// # Errors
///
/// Where the page names a repository of another namespace, which no
/// listing of `namespace` holds.
pub(crate) fn repositories(
    limit: usize,
    namespace: &str,
    body: &[u8],
) -> Result<Page, SourceError> {
    let repositories: Repositories = read(REPOSITORIES, limit, body)?;
    if let Some(foreign) = repositories
        .results
        .iter()
        .find(|repository| !repository.namespace.eq_ignore_ascii_case(namespace))
    {
        return Err(SourceError::Unreadable {
            what: REPOSITORIES,
            reason: format!(
                "it names {}/{}, outside the namespace {namespace}; check that api lists Docker Hub's namespaces",
                foreign.namespace, foreign.name
            ),
        });
    }
    Ok(Page {
        packages: repositories
            .results
            .into_iter()
            .map(|repository| {
                let visibility = if repository.is_private {
                    Visibility::Private
                } else {
                    Visibility::Public
                };
                (
                    format!("{}/{}", repository.namespace, repository.name),
                    Some(visibility),
                )
            })
            .collect(),
        more: repositories.next.is_some(),
    })
}
