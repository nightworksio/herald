use herald_core::SecretRef;

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
