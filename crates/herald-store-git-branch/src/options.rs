use herald_core::SecretRef;
use schemars::JsonSchema;
use serde::Deserialize;

/// The git-branch store's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoreOptions {
    /// The repository, as `owner/name`; the run's own where absent.
    pub repository: Option<String>,
    /// The branch the records are kept on; `herald-state` where absent.
    pub branch: Option<String>,
    /// The secret holding a token that may push to the branch;
    /// `GITHUB_TOKEN` where absent.
    pub token: Option<SecretRef>,
    /// The API's base URL, for a GitHub Enterprise Server; GitHub's own API
    /// where absent.
    pub api: Option<String>,
    /// The secret holding the key every record is sealed with, such as
    /// `HERALD_STORE_KEY`: whoever can write the repository could otherwise
    /// forge records herald trusts (ADR-0005).
    pub key: SecretRef,
}
