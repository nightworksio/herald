use herald_core::{SecretRef, path_of_names, web_url};

use crate::Registry;

/// A registry source's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegistryOptions {
    /// The package, as the registry names it: `vendor/name` on Packagist,
    /// a crate's name, an npm package's name such as `@scope/name`, or a
    /// Docker Hub repository such as `library/caddy`, where an official
    /// image may leave out `library/`.
    pub package: String,
    /// The registry's base URL, for a mirror or a private registry that
    /// answers as the public one does; the public registry's where absent.
    pub api: Option<String>,
    /// A token for a private npm package, sent as a bearer token; the other
    /// registries take none.
    pub token: Option<SecretRef>,
    /// How many of the newest versions the first poll announces; none where
    /// absent (ADR-0003).
    #[serde(default)]
    pub backfill: usize,
}

impl RegistryOptions {
    /// Whether the options are sound for a source on `registry`.
    ///
    /// # Errors
    ///
    /// When `package` is not a name the registry gives a package, or `api`
    /// is not a web address.
    pub fn check(&self, registry: Registry) -> Result<(), String> {
        let (path, count) = match (registry, self.package.strip_prefix('@')) {
            (Registry::Npm, Some(scoped)) => (scoped, 2..=2),
            _ => (self.package.as_str(), registry.names()),
        };
        if !path_of_names(path, count) {
            return Err(format!(
                "\"{}\" is not a package; give it as {}, of letters, digits, '.', '_' and '-'",
                self.package,
                registry.shape()
            ));
        }
        match &self.api {
            Some(api) if !web_url(api) => Err(format!(
                "\"{api}\" is not a web address; give the registry's base URL, such as https://registry.example"
            )),
            _ => Ok(()),
        }
    }
}
