//! A delivery saying an owner's repositories joined, left or changed, which
//! has the owner listed again (ADR-0041).

use herald_wire::SourceError;
use serde::Deserialize;

use crate::Forge;
use crate::wire::read;

/// What a delivery saying an owner's repositories changed is read as.
pub(crate) const MOVED: &str = "a repository event";

/// An account, as a GitHub installation names it.
#[derive(Deserialize)]
struct Account {
    login: String,
}

/// A GitHub App's installation, by the account it is installed on.
#[derive(Deserialize)]
struct Installed {
    account: Account,
}

/// A repository, by its full path.
#[derive(Deserialize)]
struct Named {
    full_name: String,
}

/// A repository event, by its repository, or an installation's change of
/// repositories, by its installation.
#[derive(Deserialize)]
struct Moved {
    repository: Option<Named>,
    installation: Option<Installed>,
}

/// The events of `forge`'s deliveries that say the owner's repositories
/// changed; none on GitLab.
const fn named(forge: Forge) -> &'static [&'static str] {
    match forge {
        Forge::GitHub => &["repository", "installation_repositories"],
        Forge::Forgejo => &["repository"],
        Forge::GitLab => &[],
    }
}

/// The path of the owner whose repositories a delivery of `forge` with
/// `headers` says joined, left or changed, read from `payload`, where it is
/// such a delivery: the owner of its repository, or the account its
/// installation is on.
pub(crate) fn moved(
    forge: Forge,
    headers: &[(&str, &str)],
    payload: &[u8],
) -> Option<Result<String, SourceError>> {
    forge
        .event_name(headers)
        .filter(|name| named(forge).contains(name))?;
    Some(read::<Moved>(MOVED, payload).and_then(|moved| {
        match (moved.repository, moved.installation) {
            (Some(repository), _) => Ok(repository
                .full_name
                .rsplit_once('/')
                .map_or_else(String::new, |(owner, _)| owner.to_owned())),
            (None, Some(installed)) => Ok(installed.account.login),
            (None, None) => Err(SourceError::Unreadable {
                what: MOVED,
                reason: "it names no repository and no installation".to_owned(),
            }),
        }
    }))
}

#[cfg(test)]
mod tests;
