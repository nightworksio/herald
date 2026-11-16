use std::collections::BTreeMap;

use herald_core::unreserved;
use herald_wire::{SourceError, read};
use jiff::Timestamp;
use serde::Deserialize;

use crate::namespace::Page;
use crate::package::{Api, RegistryVersion};

/// Where Packagist lists a vendor's packages where `listing_api` is absent.
pub(crate) const LISTING: &str = "https://packagist.org";

/// What a vendor's listing is called.
pub(crate) const VENDOR: &str = "a vendor's packages";

/// The path of a Composer repository's root, which names its packages.
pub(crate) const COMPOSER: &str = "/packages.json";

#[derive(Deserialize)]
struct Vendor {
    #[serde(rename = "packageNames")]
    names: Vec<String>,
}

#[derive(Deserialize)]
struct Composer {
    #[serde(rename = "available-packages")]
    available: Option<Vec<String>>,
}

pub(crate) const API: Api = Api {
    url: "https://repo.packagist.org",
    limit: 16_777_216,
    versions: "Packagist's versions",
};

/// The format whose entries leave out what the entry before gave.
const MINIFIED: &str = "composer/2.0";

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
    let name: Vec<String> = name.split('/').map(unreserved).collect();
    format!("/p2/{}.json", name.join("/"))
}

pub(crate) fn page(name: &str, version: &str) -> String {
    format!("https://packagist.org/packages/{name}#{version}")
}

/// Each tagged version of `name` with its time, a minified entry taking the
/// time of the entry before where it gives none. A version whose time is not
/// one, Composer's `__unset` among them, is left out.
pub(crate) fn found(
    api: &Api,
    limit: usize,
    name: &str,
    body: &[u8],
) -> Result<Vec<RegistryVersion>, SourceError> {
    let metadata: Metadata = read(api.versions, limit, body)?;
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
            Some(given) => time = Some(given),
            None if minified => {}
            None => time = None,
        }
        if let Some(published) = time
            .as_deref()
            .and_then(|time| time.parse::<Timestamp>().ok())
        {
            found.push(RegistryVersion {
                version: entry.version,
                published,
            });
        }
    }
    Ok(found)
}

/// The path that lists `vendor`'s packages on Packagist.
pub(crate) fn vendor_path(vendor: &str) -> String {
    format!("/packages/list.json?vendor={}", unreserved(vendor))
}

/// The packages Packagist lists for `vendor`.
///
/// # Errors
///
/// Where the listing names a package of another vendor, which no listing of
/// `vendor` holds.
pub(crate) fn vendor_packages(
    limit: usize,
    vendor: &str,
    body: &[u8],
) -> Result<Page, SourceError> {
    let listed: Vendor = read(VENDOR, limit, body)?;
    let prefix = format!("{}/", vendor.to_ascii_lowercase());
    if let Some(foreign) = listed
        .names
        .iter()
        .find(|name| !name.to_ascii_lowercase().starts_with(&prefix))
    {
        return Err(SourceError::Unreadable {
            what: VENDOR,
            reason: format!(
                "it names {foreign}, outside the vendor {vendor}; check that listing_api lists Packagist's vendors"
            ),
        });
    }
    Ok(Page {
        packages: listed.names.into_iter().map(|name| (name, None)).collect(),
        more: false,
    })
}

/// The packages of `vendor` a Composer repository names as available.
///
/// # Errors
///
/// Where the repository names none, as one that names its packages by
/// pattern does.
pub(crate) fn composer_packages(
    limit: usize,
    vendor: &str,
    body: &[u8],
) -> Result<Page, SourceError> {
    let composer: Composer = read(VENDOR, limit, body)?;
    let available = composer.available.ok_or_else(|| SourceError::Unreadable {
        what: VENDOR,
        reason: "the Composer repository names no available-packages; name its packages one by one with package".to_owned(),
    })?;
    let prefix = format!("{}/", vendor.to_ascii_lowercase());
    Ok(Page {
        packages: available
            .into_iter()
            .filter(|name| name.to_ascii_lowercase().starts_with(&prefix))
            .map(|name| (name, None))
            .collect(),
        more: false,
    })
}
