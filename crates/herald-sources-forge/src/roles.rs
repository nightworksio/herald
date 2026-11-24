//! A person's role on a repository, as each forge names it.

use herald_core::Permission;
use serde::Deserialize;

/// The least GitLab access level of each permission: Developer,
/// Maintainer and Owner.
const GITLAB_LEVELS: [(Permission, u64); 3] = [
    (Permission::Write, 30),
    (Permission::Maintain, 40),
    (Permission::Admin, 50),
];

/// What GitHub says a person may do on a repository.
#[derive(Debug, Deserialize, Default)]
pub(crate) struct GithubPermissions {
    #[serde(default)]
    admin: bool,
    #[serde(default)]
    maintain: bool,
    #[serde(default)]
    push: bool,
}

impl GithubPermissions {
    /// The role they make, none below write.
    pub(crate) const fn role(&self) -> Option<Permission> {
        if self.admin {
            Some(Permission::Admin)
        } else if self.maintain {
            Some(Permission::Maintain)
        } else if self.push {
            Some(Permission::Write)
        } else {
            None
        }
    }
}

/// The role GitLab's access level `level` makes, none below Developer.
pub(crate) fn gitlab_role(level: u64) -> Option<Permission> {
    GITLAB_LEVELS
        .iter()
        .rev()
        .find(|(_, least)| level >= *least)
        .map(|(permission, _)| *permission)
}

/// The role Forgejo's or Gitea's permission `named` makes, none below
/// write: they have no maintain role, so an admin holds it.
pub(crate) fn forgejo_role(named: &str) -> Option<Permission> {
    match named {
        "owner" | "admin" => Some(Permission::Admin),
        "write" => Some(Permission::Write),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
