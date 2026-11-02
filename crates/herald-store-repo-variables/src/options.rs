use herald_core::SecretRef;
use schemars::JsonSchema;
use serde::Deserialize;

/// The repository-variables store's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoreOptions {
    /// The secret holding a token with the repository's Variables
    /// permission; the job's own token cannot write variables.
    pub token: SecretRef,
    /// The repository, as `owner/name`; the run's own where absent.
    pub repository: Option<String>,
    /// The API's base URL, for a GitHub Enterprise Server; GitHub's own API
    /// where absent.
    pub api: Option<String>,
}
