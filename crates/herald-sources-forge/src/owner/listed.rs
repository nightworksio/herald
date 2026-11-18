//! Repositories as each forge lists them, read as herald's `Listed`.

use std::collections::BTreeMap;

use herald_core::{Listed, Visibility};
use serde::Deserialize;

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
pub(super) struct GitLab {
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
pub(super) struct Forgejo {
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
