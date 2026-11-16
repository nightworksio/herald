use std::collections::BTreeMap;
use std::fmt;

use herald_core::unreserved;
use herald_ports::Outbound;
use herald_wire::{SourceError, read};
use jiff::Timestamp;
use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

use crate::namespace::Page;
use crate::package::{Api, RegistryVersion};

/// What npm's listing of an organization's packages is called.
pub(crate) const ORG: &str = "npm's organization packages";

/// An organization's packages, each named with what the organization may
/// do with it, which is skipped as it is read.
#[derive(Deserialize)]
#[serde(transparent)]
struct Org {
    #[serde(deserialize_with = "keys")]
    names: Vec<String>,
}

pub(crate) const API: Api = Api {
    url: "https://registry.npmjs.org",
    limit: Outbound::BODY,
    versions: "npm's packument",
};

/// The packument's versions and their times; everything else in it is
/// skipped as it is read.
#[derive(Deserialize)]
struct Packument {
    #[serde(default, deserialize_with = "keys")]
    versions: Vec<String>,
    #[serde(default)]
    time: BTreeMap<String, Timestamp>,
}

/// The keys of a JSON object, each value skipped as it is read.
fn keys<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    struct Keys;

    impl<'de> Visitor<'de> for Keys {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an object of versions")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Vec<String>, A::Error> {
            let mut keys = Vec::new();
            while let Some((key, IgnoredAny)) = map.next_entry::<String, IgnoredAny>()? {
                keys.push(key);
            }
            Ok(keys)
        }
    }

    deserializer.deserialize_map(Keys)
}

pub(crate) fn path(name: &str) -> String {
    match name.strip_prefix('@') {
        Some(scoped) => format!("/@{}", unreserved(scoped)),
        None => format!("/{}", unreserved(name)),
    }
}

pub(crate) fn page(name: &str, version: &str) -> String {
    format!("https://www.npmjs.com/package/{name}/v/{version}")
}

/// Each published version with its time; a version unpublished since keeps
/// its time but leaves the versions, and is not found.
pub(crate) fn found(
    api: &Api,
    limit: usize,
    body: &[u8],
) -> Result<Vec<RegistryVersion>, SourceError> {
    let packument: Packument = read(api.versions, limit, body)?;
    let time = packument.time;
    Ok(packument
        .versions
        .into_iter()
        .filter_map(|version| {
            time.get(&version).map(|published| RegistryVersion {
                version,
                published: *published,
            })
        })
        .collect())
}

/// The path that lists the packages of the organization behind `scope`.
pub(crate) fn org_path(scope: &str) -> String {
    format!(
        "/-/org/{}/package",
        unreserved(scope.trim_start_matches('@'))
    )
}

/// The packages the organization behind `scope` lists: those of the scope,
/// and its others too where `unscoped` says so.
pub(crate) fn org_packages(
    limit: usize,
    scope: &str,
    unscoped: bool,
    body: &[u8],
) -> Result<Page, SourceError> {
    let org: Org = read(ORG, limit, body)?;
    let prefix = format!("{}/", scope.to_ascii_lowercase());
    Ok(Page {
        packages: org
            .names
            .into_iter()
            .filter(|name| {
                name.to_ascii_lowercase().starts_with(&prefix)
                    || (unscoped && !name.starts_with('@'))
            })
            .map(|name| (name, None))
            .collect(),
        more: false,
    })
}
