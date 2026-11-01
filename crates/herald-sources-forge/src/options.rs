use herald_core::SecretRef;

/// A forge source's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceOptions {
    /// The repository, as `owner/name`.
    pub repository: String,
    /// The API's base URL, for a GitHub Enterprise Server; GitHub's own API
    /// where absent.
    pub api: Option<String>,
    /// A token that raises the API's rate limit, such as the run's
    /// `GITHUB_TOKEN`.
    pub token: Option<SecretRef>,
    /// How many of the newest releases the first poll announces; none where
    /// absent (ADR-0003).
    #[serde(default)]
    pub backfill: usize,
}
