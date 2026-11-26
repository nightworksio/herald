//! What herald's own queue reads beside its entries: whether a pull
//! request is merged, the parents of a commit the forge itself made and
//! signed, and the methods the repository allows a merge by (ADR-0087).

use herald_core::merging::Method;
use herald_kinds::{Methods, Unpolled};
use herald_ports::{DynHttp, Erased};
use serde::Deserialize;

use super::{ForgeMergeReader, PULL};
use crate::Forge;

/// What a commit read is named in a refusal.
const COMMIT: &str = "the commit herald's update made";

/// What a repository's settings read is named in a refusal.
const SETTINGS: &str = "the repository's merge settings";

/// GitLab's state of a merged merge request.
const MERGED: &str = "merged";

/// GitLab's squash setting that allows no squash.
const NEVER: &str = "never";

#[derive(Deserialize)]
struct Pull {
    #[serde(default)]
    merged: bool,
    #[serde(default)]
    state: String,
}

#[derive(Deserialize)]
struct Parent {
    sha: String,
}

#[derive(Deserialize)]
struct Verification {
    verified: bool,
}

#[derive(Deserialize)]
struct Committer {
    email: String,
}

#[derive(Deserialize)]
struct Signed {
    committer: Committer,
    verification: Verification,
}

#[derive(Deserialize)]
struct Commit {
    parents: Vec<Parent>,
    commit: Signed,
}

#[derive(Deserialize)]
struct GitHubSettings {
    #[serde(rename = "allow_merge_commit")]
    merge: Option<bool>,
    #[serde(rename = "allow_squash_merge")]
    squash: Option<bool>,
    #[serde(rename = "allow_rebase_merge")]
    rebase: Option<bool>,
}

#[derive(Deserialize)]
struct ForgejoSettings {
    #[serde(rename = "allow_merge_commits")]
    merge: Option<bool>,
    #[serde(rename = "allow_squash_merge")]
    squash: Option<bool>,
    #[serde(rename = "allow_rebase")]
    rebase: Option<bool>,
    #[serde(rename = "allow_fast_forward_only_merge")]
    fast_forward: Option<bool>,
}

#[derive(Deserialize)]
struct GitLabSettings {
    merge_method: Option<String>,
    squash_option: Option<String>,
}

/// Whether the pull request `number` is merged.
pub(super) async fn merged(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    number: u64,
) -> Result<bool, Unpolled> {
    let path = match reader.forge() {
        Forge::GitHub | Forge::Forgejo => format!("/pulls/{number}"),
        Forge::GitLab => format!("/merge_requests/{number}"),
    };
    let pull: Pull = reader.needed(http, reader.get(&path)?, PULL).await?;
    Ok(pull.merged || pull.state == MERGED)
}

/// The committer GitHub signs the commits it makes through its own API
/// for: `signer`, or on github.com its own where `signer` is none; none on
/// any other forge.
fn signing<'s>(reader: &ForgeMergeReader, signer: Option<&'s str>) -> Option<&'s str> {
    if reader.forge() != Forge::GitHub {
        return None;
    }
    signer.or_else(|| herald_wire::github::web_flow(reader.origin.api()))
}

/// The parents of the commit `sha` where GitHub made and signed it through
/// its own API for the committer [`signing`] names; none elsewhere.
pub(super) async fn forge_made(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
    (sha, signer): (&str, Option<&str>),
) -> Result<Option<Vec<String>>, Unpolled> {
    let Some(signer) = signing(reader, signer) else {
        return Ok(None);
    };
    let sha = crate::forge::named(sha, COMMIT)?;
    let commit: Commit = reader
        .needed(http, reader.get(&format!("/commits/{sha}"))?, COMMIT)
        .await?;
    let made = commit.commit.verification.verified && commit.commit.committer.email == signer;
    Ok(made.then(|| {
        commit
            .parents
            .into_iter()
            .map(|parent| parent.sha)
            .collect()
    }))
}

/// Each method of `allowed` the settings allow, and whether every one of
/// them was said.
fn allowing(allowed: &[(Method, Option<bool>)]) -> Methods {
    Methods {
        allowed: allowed
            .iter()
            .filter(|(_, allows)| *allows == Some(true))
            .map(|(method, _)| *method)
            .collect(),
        whole: allowed.iter().all(|(_, allows)| allows.is_some()),
    }
}

/// The methods a GitLab project's merge method and squash setting allow.
fn gitlab_methods(settings: &GitLabSettings) -> Methods {
    let method = match settings.merge_method.as_deref() {
        Some("merge") => Some(Method::Merge),
        Some("rebase_merge") => Some(Method::Rebase),
        Some("ff") => Some(Method::FastForward),
        _ => None,
    };
    let squashing = settings
        .squash_option
        .as_deref()
        .map(|option| option != NEVER);
    let mut allowed: Vec<Method> = method.into_iter().collect();
    if squashing == Some(true) {
        allowed.push(Method::Squash);
    }
    Methods {
        allowed,
        whole: method.is_some() && squashing.is_some(),
    }
}

/// The methods the repository allows a merge by.
pub(super) async fn methods(
    reader: &ForgeMergeReader,
    http: &Erased<'_, dyn DynHttp + '_>,
) -> Result<Methods, Unpolled> {
    let request = reader.get("")?;
    Ok(match reader.forge() {
        Forge::GitHub => {
            let settings: GitHubSettings = reader.needed(http, request, SETTINGS).await?;
            allowing(&[
                (Method::Merge, settings.merge),
                (Method::Squash, settings.squash),
                (Method::Rebase, settings.rebase),
            ])
        }
        Forge::Forgejo => {
            let settings: ForgejoSettings = reader.needed(http, request, SETTINGS).await?;
            allowing(&[
                (Method::Merge, settings.merge),
                (Method::Squash, settings.squash),
                (Method::Rebase, settings.rebase),
                (Method::FastForward, settings.fast_forward),
            ])
        }
        Forge::GitLab => {
            let settings: GitLabSettings = reader.needed(http, request, SETTINGS).await?;
            gitlab_methods(&settings)
        }
    })
}

#[cfg(test)]
mod tests;
