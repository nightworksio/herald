use herald_wire::{PAGE, SourceError, read, segment};
use jiff::Timestamp;
use serde::Deserialize;

use crate::package::{Api, Found};

pub(crate) const API: Api = Api {
    url: "https://hub.docker.com/v2",
    limit: 4_194_304,
    versions: "Docker Hub's tags",
};

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

pub(crate) fn path(name: &str) -> String {
    let (namespace, repository) = split(name);
    format!(
        "/namespaces/{}/repositories/{}/tags?page_size={PAGE}&ordering=last_updated",
        segment(namespace),
        segment(repository)
    )
}

pub(crate) fn page(name: &str, tag: &str) -> String {
    match split(name) {
        (OFFICIAL, image) => format!("https://hub.docker.com/_/{image}/tags?name={tag}"),
        _ => format!("https://hub.docker.com/r/{name}/tags?name={tag}"),
    }
}

/// Each tag with the time it was last pushed.
pub(crate) fn found(api: &Api, body: &[u8]) -> Result<Vec<Found>, SourceError> {
    let tags: Tags = read(api.versions, api.limit, body)?;
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
