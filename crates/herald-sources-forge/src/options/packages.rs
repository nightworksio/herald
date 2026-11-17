//! The keys that choose a forge source's packages (ADR-0035).

use std::num::NonZeroUsize;

use herald_core::{Glob, Signal, domain};

use super::{SourceOptions, globs};
use crate::Forge;

/// Who may see a package's version where its own visibility and its
/// repository's differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PackageVisibility {
    /// The stricter of the package's and the repository's: a public
    /// package of a private repository is private.
    #[default]
    Stricter,
    /// The package's own, whatever its repository's.
    Package,
}

/// The most entries a listing of packages reads where `most_packages` is
/// absent.
pub(crate) const MOST_PACKAGES: NonZeroUsize = NonZeroUsize::new(1_000).unwrap();
/// The most `most_packages` may be.
pub(crate) const MOST_PACKAGES_ALLOWED: usize = 100_000;
/// The container tags never announced where `moving` is absent.
pub(crate) const MOVING: [&str; 1] = ["latest"];
/// The types of package GitHub's packages API lists.
pub(crate) const GITHUB_PACKAGE_TYPES: [&str; 6] =
    ["npm", "maven", "rubygems", "docker", "nuget", "container"];

/// The keys that choose a source's packages.
const PACKAGED: [&str; 6] = [
    "package_types",
    "packages",
    "moving",
    "registry",
    "most_packages",
    "package_visibility",
];

/// Whether `registry` is a registry's host: a domain, and a port where
/// one follows it.
pub(crate) fn registry_host(registry: &str) -> bool {
    match registry.split_once(':') {
        Some((host, port)) => domain(host) && port.parse::<u16>().is_ok_and(|port| port > 0),
        None => domain(registry),
    }
}

impl SourceOptions {
    /// Whether the keys that choose packages are sound on `forge`, and
    /// given only where some repository the source takes announces them.
    pub(super) fn packages_check(&self, forge: Forge) -> Result<(), String> {
        let given = [
            self.package_types.is_some(),
            !self.packages.is_empty(),
            self.moving.is_some(),
            self.registry.is_some(),
            self.most_packages.is_some(),
            self.package_visibility.is_some(),
        ];
        let packaged = self.may_take(Signal::Package);
        if let Some(key) = PACKAGED
            .into_iter()
            .zip(given)
            .find_map(|(key, given)| (given && !packaged).then_some(key))
        {
            return Err(format!(
                "{key} chooses packages, which no repository the source takes announces; add \"package\" to signals, or remove {key}"
            ));
        }
        if packaged && forge == Forge::GitHub && !self.reads_with_token() {
            return Err(
                "GitHub's packages API answers only with a token that may read packages; give token, or remove \"package\" from signals".to_owned(),
            );
        }
        if let Some(types) = &self.package_types {
            if types.is_empty() {
                return Err(
                    "package_types names no type; give one, or remove it for every type".to_owned(),
                );
            }
            for kind in types {
                let known = match forge {
                    Forge::GitHub => GITHUB_PACKAGE_TYPES.contains(&kind.as_str()),
                    Forge::GitLab | Forge::Forgejo => {
                        !kind.is_empty()
                            && kind.bytes().all(|byte| {
                                byte.is_ascii_lowercase()
                                    || byte.is_ascii_digit()
                                    || matches!(byte, b'_' | b'-')
                            })
                    }
                };
                if !known {
                    return Err(match forge {
                        Forge::GitHub => format!(
                            "package_types \"{kind}\" is not a type GitHub lists; give {}",
                            GITHUB_PACKAGE_TYPES.join(", ")
                        ),
                        Forge::GitLab | Forge::Forgejo => format!(
                            "package_types \"{kind}\" is not a type; name it as the forge does, in lowercase letters, digits, '_' and '-'"
                        ),
                    });
                }
            }
        }
        globs(&self.packages).map_err(|reason| format!("packages: {reason}"))?;
        self.moving()
            .map_err(|reason| format!("moving: {reason}"))?;
        if let Some(registry) = &self.registry
            && !registry_host(registry)
        {
            return Err(format!(
                "registry \"{registry}\" is not a registry's host; give it as registry.example or registry.example:5000"
            ));
        }
        if self.most_packages().get() > MOST_PACKAGES_ALLOWED {
            return Err(format!(
                "most_packages is at most {MOST_PACKAGES_ALLOWED}; lower it"
            ));
        }
        Ok(())
    }

    /// The globs of the packages announced; every package where none.
    ///
    /// # Errors
    ///
    /// When a glob cannot be read.
    pub(crate) fn packages(&self) -> Result<Vec<Glob>, String> {
        globs(&self.packages)
    }

    /// The globs of the container tags never announced.
    ///
    /// # Errors
    ///
    /// When a glob cannot be read.
    pub(crate) fn moving(&self) -> Result<Vec<Glob>, String> {
        self.moving.as_ref().map_or_else(
            || globs(&MOVING.map(ToOwned::to_owned)),
            |moving| globs(moving),
        )
    }

    /// The most entries one listing of packages reads.
    #[must_use]
    pub(crate) fn most_packages(&self) -> NonZeroUsize {
        self.most_packages.unwrap_or(MOST_PACKAGES)
    }
}
