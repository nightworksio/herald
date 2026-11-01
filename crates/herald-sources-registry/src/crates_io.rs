use herald_wire::{PAGE, SourceError, read, segment};
use jiff::Timestamp;
use serde::Deserialize;

use crate::package::{Api, Found};

pub(crate) const API: Api = Api {
    url: "https://crates.io/api/v1",
    limit: 4_194_304,
    versions: "crates.io's versions",
};

#[derive(Deserialize)]
struct Versions {
    versions: Vec<Version>,
}

#[derive(Deserialize)]
struct Version {
    num: String,
    created_at: Timestamp,
    yanked: bool,
}

pub(crate) fn path(name: &str) -> String {
    format!(
        "/crates/{}/versions?per_page={PAGE}&sort=date",
        segment(name)
    )
}

pub(crate) fn page(name: &str, version: &str) -> String {
    format!("https://crates.io/crates/{name}/{version}")
}

/// Each version of the crate that is not yanked.
pub(crate) fn found(api: &Api, body: &[u8]) -> Result<Vec<Found>, SourceError> {
    let versions: Versions = read(api.versions, api.limit, body)?;
    Ok(versions
        .versions
        .into_iter()
        .filter(|version| !version.yanked)
        .map(|version| Found {
            version: version.num,
            published: version.created_at,
        })
        .collect())
}
