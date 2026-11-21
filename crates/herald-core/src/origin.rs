use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::address::{domain, path_of_names};
use crate::event::Problem;
use crate::members::{Listed, Visibility, any_of, holds_all};
use crate::queue::MOST_AREAS;
use crate::route::SourceName;

/// Where an event comes from: the forge or registry, the owner and the
/// repository or package, with what the source knows of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    /// The forge or registry, as a source's `kind` names it, such as
    /// `github` or `npm`.
    #[schemars(example = Origin::FORGE)]
    pub forge: String,
    /// Its host, with a port where it is not the scheme's own.
    #[schemars(example = Origin::HOST)]
    pub host: String,
    /// The owner: an account, organization or group path, or a registry's
    /// scope, vendor or namespace; empty where the package has none.
    #[serde(default)]
    pub owner: String,
    /// The repository's full path, or the package's name.
    #[schemars(example = Origin::REPOSITORY)]
    pub repository: String,
    /// The forge's or registry's stable id for it.
    pub id: String,
    /// Its topics.
    #[serde(default)]
    pub topics: Vec<String>,
    /// Its custom properties, each with the values it holds.
    #[serde(default)]
    pub properties: BTreeMap<String, Vec<String>>,
    /// Who may see it.
    pub visibility: Visibility,
    /// The areas of its repository the pull request it is of falls in,
    /// where every path that pull request's head changes is known; none
    /// where they are not, which a route narrowing by areas never takes
    /// (ADR-0072).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub areas: Option<Vec<String>>,
    /// The source or app it reached herald through, which herald names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(skip)]
    pub source: Option<SourceName>,
}

/// The most names an origin's path holds: a GitLab project nested in
/// subgroups.
const NAMES: usize = 21;

/// The most bytes one of an origin's fields holds.
const FIELD: usize = 512;

/// Whether `text` is a name of lowercase ASCII letters, digits and inner
/// `-`, as a kind of source or a GitHub App's table is named.
#[must_use]
pub fn kind_name(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('-')
        && !text.ends_with('-')
        && text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// Whether `text` is a host, with a port where it gives one.
fn host(text: &str) -> bool {
    let (name, port) = text.split_once(':').unwrap_or((text, ""));
    (domain(name) || name == "localhost")
        && (port.is_empty() || port.parse::<u16>().is_ok_and(|port| port > 0))
}

impl Origin {
    const FORGE: &str = "github";
    const HOST: &str = "github.com";
    const REPOSITORY: &str = "nightworksio/herald";

    /// The origin of the member `listed` of an owner on the forge `forge`
    /// at `host`.
    #[must_use]
    pub fn of_member(forge: &str, host: &str, listed: &Listed) -> Self {
        Self {
            forge: forge.to_owned(),
            host: host.to_owned(),
            owner: listed.owner().to_owned(),
            repository: listed.path.clone(),
            id: listed.id.clone(),
            topics: listed.topics.clone(),
            properties: listed.properties.clone(),
            visibility: listed.visibility,
            areas: None,
            source: None,
        }
    }

    /// The origin of the package `package` of `owner` on the public
    /// registry `registry` at `host`.
    #[must_use]
    pub fn of_package(registry: &str, host: &str, owner: &str, package: &str) -> Self {
        Self {
            forge: registry.to_owned(),
            host: host.to_owned(),
            owner: owner.to_owned(),
            repository: package.to_owned(),
            id: package.to_owned(),
            topics: Vec::new(),
            properties: BTreeMap::new(),
            visibility: Visibility::Public,
            areas: None,
            source: None,
        }
    }

    /// The origin as an app wrote it, where each field is one a source
    /// could give.
    ///
    /// # Errors
    ///
    /// The [`Problem`] of the first field no source could give.
    pub fn checked(self) -> Result<Self, Problem> {
        let paths = 1..=NAMES;
        let long = [
            &self.forge,
            &self.host,
            &self.owner,
            &self.repository,
            &self.id,
        ]
        .into_iter()
        .chain(&self.topics)
        .chain(self.properties.keys())
        .chain(self.properties.values().flatten())
        .chain(self.areas.iter().flatten())
        .any(|field| field.len() > FIELD);
        if long {
            Err(Problem::OriginTooLong)
        } else if !kind_name(&self.forge) {
            Err(Problem::OriginForge)
        } else if !host(&self.host) {
            Err(Problem::OriginHost)
        } else if !(self.owner.is_empty() || path_of_names(&self.owner, paths.clone()))
            || !path_of_names(self.repository.trim_start_matches('@'), paths)
        {
            Err(Problem::OriginPath)
        } else if self.id.trim().is_empty() {
            Err(Problem::OriginId)
        } else if self
            .areas
            .as_ref()
            .is_some_and(|areas| areas.len() > MOST_AREAS)
        {
            Err(Problem::OriginAreas)
        } else {
            Ok(self)
        }
    }

    /// Whether it holds one of `wanted`'s topics, ignoring case, or
    /// `wanted` is empty.
    #[must_use]
    pub fn holds_topics(&self, wanted: &[String]) -> bool {
        any_of(wanted, self.topics.iter().map(String::as_str))
    }

    /// Whether it holds every property of `wanted` with one of the values
    /// given for it, ignoring case, any value where none is given.
    #[must_use]
    pub fn holds_properties(&self, wanted: &BTreeMap<String, Vec<String>>) -> bool {
        holds_all(wanted, &self.properties)
    }
}

#[cfg(test)]
pub(crate) mod tests;
