//! The repository a webhook delivery is of, as an owner source's filters
//! read it (ADR-0030).

use std::collections::BTreeMap;

use herald_core::{Delivered, Origin, Visibility};
use herald_wire::SourceError;
use serde::Deserialize;

use crate::Forge;
use crate::owner::{Property, flagged, levelled, named, properties};
use crate::wire::read;

/// A repository, as GitHub's and Forgejo's deliveries describe it.
#[derive(Deserialize)]
struct Described {
    id: u64,
    name: String,
    full_name: String,
    topics: Option<Vec<String>>,
    custom_properties: Option<BTreeMap<String, Option<Property>>>,
    visibility: Option<String>,
    private: Option<bool>,
    internal: Option<bool>,
    archived: Option<bool>,
    fork: Option<bool>,
    is_template: Option<bool>,
    template: Option<bool>,
    language: Option<String>,
}

/// A GitHub or Forgejo delivery, by the repository it is of.
#[derive(Deserialize)]
struct Repositoried {
    repository: Described,
}

/// A Forgejo run's delivery, by the repository its run is of.
#[derive(Deserialize)]
struct Ran {
    run: Repositoried,
}

/// A project, as GitLab's release hook describes it.
#[derive(Deserialize)]
struct Project {
    id: u64,
    path_with_namespace: String,
    visibility_level: Option<u8>,
}

/// A repository, as a forge's API describes it: GitHub's and Forgejo's
/// `full_name`, or GitLab's `path_with_namespace`.
#[derive(Deserialize)]
struct Answered {
    id: u64,
    #[serde(alias = "path_with_namespace")]
    full_name: Option<String>,
    #[serde(default)]
    topics: Vec<String>,
    custom_properties: Option<BTreeMap<String, Option<Property>>>,
    visibility: Option<String>,
    private: Option<bool>,
    internal: Option<bool>,
}

/// A GitLab delivery, by the project it is of.
#[derive(Deserialize)]
struct Projected {
    project: Project,
}

/// The repository the forge's API answered with in `body`, as its id and
/// what it names of it, at `path` where the answer names no path.
fn answered(
    what: &'static str,
    body: &[u8],
    path: &str,
) -> Result<(String, Delivered), SourceError> {
    let answered = read::<Answered>(what, body)?;
    let visibility = answered.visibility.as_deref().map(named).or_else(|| {
        answered
            .private
            .map(|private| flagged(Some(private), answered.internal))
    });
    let path = answered.full_name.unwrap_or_else(|| path.to_owned());
    Ok((
        answered.id.to_string(),
        Delivered {
            path,
            topics: Some(answered.topics),
            properties: answered.custom_properties.map(properties),
            visibility,
            ..Delivered::default()
        },
    ))
}

/// Where the events of the repository `delivered`, of the id `id`, on
/// `forge` at `host` come from: private where the forge does not say who
/// may see it.
pub(crate) fn origin(forge: Forge, host: &str, (id, delivered): (String, Delivered)) -> Origin {
    let owner = delivered
        .path
        .rsplit_once('/')
        .map_or_else(String::new, |(owner, _)| owner.to_owned());
    Origin {
        forge: forge.kind().to_owned(),
        host: host.to_owned(),
        owner,
        repository: delivered.path,
        id,
        topics: delivered.topics.unwrap_or_default(),
        properties: delivered.properties.unwrap_or_default(),
        visibility: delivered.visibility.unwrap_or(Visibility::Private),
        areas: None,
        source: None,
    }
}

/// Where the events of the repository at `path` the forge's API answered
/// with in `body` come from, on `forge` at `host`.
///
/// # Errors
///
/// When the answer is not a repository.
pub(crate) fn answered_origin(
    forge: Forge,
    (host, what, path): (&str, &'static str, &str),
    body: &[u8],
) -> Result<Origin, SourceError> {
    Ok(origin(forge, host, answered(what, body, path)?))
}

/// The last name of the full path `path`.
fn last(path: &str) -> String {
    path.rsplit('/').next().unwrap_or_default().to_owned()
}

/// A GitHub or Forgejo repository, by its id and what it names of itself.
fn github(described: Described) -> (String, Delivered) {
    let visibility = described.visibility.as_deref().map(named).or_else(|| {
        described
            .private
            .map(|private| flagged(Some(private), described.internal))
    });
    (
        described.id.to_string(),
        Delivered {
            name: described.name,
            path: described.full_name,
            topics: described.topics,
            properties: described.custom_properties.map(properties),
            visibility,
            archived: described.archived,
            fork: described.fork,
            template: described.is_template.or(described.template),
            language: described.language,
        },
    )
}

/// The forge's id of the repository the delivery `payload` is of, and
/// the repository as the delivery describes it.
pub(crate) fn delivered(
    forge: Forge,
    what: &'static str,
    payload: &[u8],
) -> Result<(String, Delivered), SourceError> {
    Ok(match forge {
        Forge::GitHub | Forge::Forgejo => github(read::<Repositoried>(what, payload)?.repository),
        Forge::GitLab => {
            let project = read::<Projected>(what, payload)?.project;
            let name = last(&project.path_with_namespace);
            (
                project.id.to_string(),
                Delivered {
                    name,
                    visibility: project.visibility_level.map(|level| levelled(Some(level))),
                    path: project.path_with_namespace,
                    ..Delivered::default()
                },
            )
        }
    })
}

/// The forge's id of the repository the run delivery `payload` is of, and
/// the repository as the delivery describes it: Forgejo names it in its
/// run.
pub(crate) fn delivered_run(
    forge: Forge,
    what: &'static str,
    payload: &[u8],
) -> Result<(String, Delivered), SourceError> {
    match forge {
        Forge::Forgejo => Ok(github(read::<Ran>(what, payload)?.run.repository)),
        Forge::GitHub | Forge::GitLab => delivered(forge, what, payload),
    }
}

#[cfg(test)]
mod tests;
