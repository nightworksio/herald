//! The repository a webhook delivery is of, as an owner source's filters
//! read it (ADR-0030).

use std::collections::BTreeMap;

use herald_core::Delivered;
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

/// A project, as GitLab's release hook describes it.
#[derive(Deserialize)]
struct Project {
    id: u64,
    path_with_namespace: String,
    visibility_level: Option<u8>,
}

/// A GitLab delivery, by the project it is of.
#[derive(Deserialize)]
struct Projected {
    project: Project,
}

/// The forge's id of the repository the delivery `payload` is of, and
/// the repository as the delivery describes it.
pub(crate) fn delivered(
    forge: Forge,
    what: &'static str,
    payload: &[u8],
) -> Result<(String, Delivered), SourceError> {
    Ok(match forge {
        Forge::GitHub | Forge::Forgejo => {
            let described = read::<Repositoried>(what, payload)?.repository;
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
        Forge::GitLab => {
            let project = read::<Projected>(what, payload)?.project;
            let name = project
                .path_with_namespace
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned();
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

#[cfg(test)]
mod tests;
