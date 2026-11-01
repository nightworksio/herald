use std::collections::BTreeMap;
use std::fmt;

use herald_wire::{SourceError, read, segment};
use jiff::Timestamp;
use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

use crate::package::{Api, Found};

pub(crate) const API: Api = Api {
    url: "https://registry.npmjs.org",
    limit: 33_554_432,
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
        Some(scoped) => format!("/@{}", segment(scoped)),
        None => format!("/{}", segment(name)),
    }
}

pub(crate) fn page(name: &str, version: &str) -> String {
    format!("https://www.npmjs.com/package/{name}/v/{version}")
}

/// Each published version with its time; a version unpublished since keeps
/// its time but leaves the versions, and is not found.
pub(crate) fn found(api: &Api, body: &[u8]) -> Result<Vec<Found>, SourceError> {
    let packument: Packument = read(api.versions, api.limit, body)?;
    let time = packument.time;
    Ok(packument
        .versions
        .into_iter()
        .filter_map(|version| {
            time.get(&version).map(|published| Found {
                version,
                published: *published,
            })
        })
        .collect())
}
