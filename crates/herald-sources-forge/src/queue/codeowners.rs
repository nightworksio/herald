//! A repository's CODEOWNERS file read from its forge, and a GitLab
//! top-level `@name` looked up as a group or a person (ADR-0073).

use herald_core::{AreaKind, Codeowners, Dialect, status, unreserved};
use herald_kinds::{Codeowning, Found, Unpolled};
use herald_ports::{DynHttp, Erased, Http as _};
use herald_wire::answered;

use crate::{Forge, Origin, Token};

/// What a CODEOWNERS file is called in an error.
const FILE: &str = "CODEOWNERS file";

/// What GitLab's listing of users by name is called in an error.
const USERS: &str = "user lookup";

/// What GitLab's group is called in an error.
const GROUP: &str = "group lookup";

/// The dialect `forge` writes its CODEOWNERS files in.
pub(crate) const fn dialect(forge: Forge) -> Dialect {
    match forge {
        Forge::GitHub => Dialect::GitHub,
        Forge::GitLab => Dialect::GitLab,
        Forge::Forgejo => Dialect::Forgejo,
    }
}

/// The CODEOWNERS file of the repository at `origin`, read with `token`
/// at `reference` or the default branch, from the first of `codeowning`'s
/// places it is found at, asked through `http`; none where it is at none.
///
/// # Errors
///
/// Where a place cannot be asked, or the forge answers other than with
/// the file or that it is not there.
pub(crate) async fn found(
    http: &Erased<'_, dyn DynHttp + '_>,
    (origin, token): (&Origin, Option<&Token>),
    reference: Option<&str>,
    codeowning: &Codeowning,
) -> Result<Option<Found>, Unpolled> {
    for path in &codeowning.paths {
        let asked = origin.file_request((path, reference), token, None)?;
        let answer = http.send(asked).await.map_err(Unpolled::unreachable)?;
        if answer.status == status::NOT_FOUND {
            continue;
        }
        answered(FILE, &answer).map_err(Unpolled::unreadable)?;
        return Ok(Some(Found {
            path: path.clone(),
            file: Codeowners::within(dialect(origin.forge()), &answer.body, codeowning.most),
        }));
    }
    Ok(None)
}

/// Whether `name` is a GitLab namespace's path: letters, digits, `_`,
/// `.` and `-`, so a lookup of it names nothing else.
fn namespaced(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_.-".contains(character))
}

/// What the GitLab top-level `@name` `written` is, by GitLab's listing of
/// users by name and its group of that path, read with `token` under the
/// API of `origin` through `http`: a person, a group, or of kind name
/// where GitLab tells neither or refuses to.
///
/// # Errors
///
/// Where GitLab cannot be reached.
pub(crate) async fn named(
    http: &Erased<'_, dyn DynHttp + '_>,
    (origin, token): (&Origin, Option<&Token>),
    written: &str,
) -> Result<AreaKind, Unpolled> {
    let name = written.strip_prefix('@').unwrap_or(written);
    if origin.forge() != Forge::GitLab || !namespaced(name) {
        return Ok(AreaKind::Name);
    }
    let users = origin.rooted(&format!("/users?username={}", unreserved(name)), token);
    let answer = http.send(users).await.map_err(Unpolled::unreachable)?;
    let listed = answered(USERS, &answer).is_ok()
        && serde_json::from_slice::<Vec<serde::de::IgnoredAny>>(&answer.body)
            .is_ok_and(|users| !users.is_empty());
    if listed {
        return Ok(AreaKind::Person);
    }
    let group = origin.rooted(&format!("/groups/{}", unreserved(name)), token);
    let answer = http.send(group).await.map_err(Unpolled::unreachable)?;
    Ok(if answered(GROUP, &answer).is_ok() {
        AreaKind::Group
    } else {
        AreaKind::Name
    })
}

#[cfg(test)]
mod tests;
