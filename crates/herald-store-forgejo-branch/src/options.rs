use herald_core::SecretRef;
use schemars::JsonSchema;
use serde::Deserialize;

/// The forgejo-branch store's options in `herald.toml` (ADR-0006,
/// ADR-0090).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoreOptions {
    /// The repository, as `owner/name`; the run's own where absent.
    pub repository: Option<String>,
    /// The branch the records are kept on; `herald-state` where absent.
    /// herald's first write makes it in an empty repository; in one that
    /// holds other branches it is made once, with no history, such as with
    /// `git switch --orphan herald-state`.
    pub branch: Option<String>,
    /// The secret holding a token that may push to the branch;
    /// `GITHUB_TOKEN`, as Forgejo Actions sets it, where absent.
    pub token: Option<SecretRef>,
    /// The API's base URL, such as a Forgejo's
    /// `https://forge.example/api/v1`. Where absent, it is the run's own,
    /// as Forgejo Actions sets `GITHUB_API_URL`, when the store names no
    /// repository, and Codeberg's when it names one.
    pub api: Option<String>,
    /// The secret holding the key every record is sealed with, such as
    /// `HERALD_STORE_KEY`: whoever can write the repository could otherwise
    /// forge records herald trusts (ADR-0005).
    pub key: SecretRef,
}
