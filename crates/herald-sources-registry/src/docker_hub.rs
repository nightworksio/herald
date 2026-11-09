use herald_wire::{SourceError, read, segment};
use jiff::Timestamp;
use serde::Deserialize;

use crate::options::TagOrdering;
use crate::package::{Api, Found};

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
        segment(namespace),
        segment(repository),
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
pub(crate) fn found(api: &Api, limit: usize, body: &[u8]) -> Result<Vec<Found>, SourceError> {
    let tags: Tags = read(api.versions, limit, body)?;
    Ok(tags
        .results
        .into_iter()
        .filter_map(|tag| {
            tag.pushed.or(tag.updated).map(|published| Found {
                version: tag.name,
                published,
            })
        })
        .collect())
}
