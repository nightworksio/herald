use std::collections::BTreeMap;

use herald_wire::{SourceError, read, segment};
use jiff::Timestamp;
use serde::Deserialize;

use crate::package::{Api, Found};

pub(crate) const API: Api = Api {
    url: "https://repo.packagist.org",
    limit: 16_777_216,
    versions: "Packagist's versions",
};

/// The format whose entries leave out what the entry before gave.
const MINIFIED: &str = "composer/2.0";
/// What a minified entry gives for a key the entry before set and it does
/// not.
const UNSET: &str = "__unset";

#[derive(Deserialize)]
struct Metadata {
    minified: Option<String>,
    packages: BTreeMap<String, Vec<Entry>>,
}

#[derive(Deserialize)]
struct Entry {
    version: String,
    time: Option<String>,
}

pub(crate) fn path(name: &str) -> String {
    let name: Vec<String> = name.split('/').map(segment).collect();
    format!("/p2/{}.json", name.join("/"))
}

pub(crate) fn page(name: &str, version: &str) -> String {
    format!("https://packagist.org/packages/{name}#{version}")
}

/// Each tagged version of `name` with its time, a minified entry taking the
/// time of the entry before where it gives none.
pub(crate) fn found(api: &Api, name: &str, body: &[u8]) -> Result<Vec<Found>, SourceError> {
    let metadata: Metadata = read(api.versions, api.limit, body)?;
    let minified = metadata.minified.as_deref() == Some(MINIFIED);
    let entries = metadata
        .packages
        .into_iter()
        .find(|(listed, _)| listed.eq_ignore_ascii_case(name))
        .map(|(_, entries)| entries)
        .unwrap_or_default();
    let mut time: Option<String> = None;
    let mut found = Vec::new();
    for entry in entries {
        match entry.time {
            Some(given) if given == UNSET => time = None,
            Some(given) => time = Some(given),
            None if minified => {}
            None => time = None,
        }
        if let Some(published) = time
            .as_deref()
            .and_then(|time| time.parse::<Timestamp>().ok())
        {
            found.push(Found {
                version: entry.version,
                published,
            });
        }
    }
    Ok(found)
}
