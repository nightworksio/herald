use herald_core::unreserved;
use herald_wire::{SourceError, read};
use jiff::Timestamp;
use serde::Deserialize;

use crate::namespace::Page;
use crate::options::CratesOwner;
use crate::package::{Api, RegistryVersion};

/// What crates.io's answer naming an owner is called.
pub(crate) const OWNER: &str = "crates.io's owner";

/// What crates.io's listing of an owner's crates is called.
pub(crate) const CRATES: &str = "crates.io's crates of an owner";

#[derive(Deserialize)]
struct Owned {
    user: Option<Id>,
    team: Option<Id>,
}

#[derive(Deserialize)]
struct Id {
    id: u64,
}

#[derive(Deserialize)]
struct Crates {
    crates: Vec<Named>,
    meta: Meta,
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

#[derive(Deserialize)]
struct Meta {
    next_page: Option<String>,
}

pub(crate) const API: Api = Api {
    url: "https://crates.io/api/v1",
    limit: herald_wire::PARSE_LIMIT,
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

pub(crate) fn path(name: &str, page: usize) -> String {
    format!(
        "/crates/{}/versions?per_page={page}&sort=date",
        unreserved(name)
    )
}

pub(crate) fn page(name: &str, version: &str) -> String {
    format!("https://crates.io/crates/{name}/{version}")
}

/// Each version of the crate that is not yanked.
pub(crate) fn found(
    api: &Api,
    limit: usize,
    body: &[u8],
) -> Result<Vec<RegistryVersion>, SourceError> {
    let versions: Versions = read(api.versions, limit, body)?;
    Ok(versions
        .versions
        .into_iter()
        .filter(|version| !version.yanked)
        .map(|version| RegistryVersion {
            version: version.num,
            published: version.created_at,
        })
        .collect())
}

/// The path that names `owner`.
pub(crate) fn owner_path(owner: &CratesOwner<'_>) -> String {
    match owner {
        CratesOwner::User(login) => format!("/users/{}", unreserved(login)),
        CratesOwner::Team(team) => format!("/teams/{}", unreserved(team)),
    }
}

/// The id crates.io gives the owner it names.
pub(crate) fn owner_id(limit: usize, body: &[u8]) -> Result<u64, SourceError> {
    let owned: Owned = read(OWNER, limit, body)?;
    owned
        .user
        .or(owned.team)
        .map(|owner| owner.id)
        .ok_or_else(|| SourceError::Unreadable {
            what: OWNER,
            reason: "it names no user or team".to_owned(),
        })
}

/// The path of page `number` of `owner`'s crates, whose id is `id`, `page`
/// to a page.
pub(crate) fn crates_path(owner: CratesOwner<'_>, id: u64, page: usize, number: usize) -> String {
    let key = match owner {
        CratesOwner::User(_) => "user_id",
        CratesOwner::Team(_) => "team_id",
    };
    format!("/crates?{key}={id}&per_page={page}&page={number}")
}

/// The crates a page of an owner's listing names.
pub(crate) fn crates(limit: usize, body: &[u8]) -> Result<Page, SourceError> {
    let crates: Crates = read(CRATES, limit, body)?;
    Ok(Page {
        packages: crates
            .crates
            .into_iter()
            .map(|named| (named.name, None))
            .collect(),
        more: crates.meta.next_page.is_some(),
    })
}
