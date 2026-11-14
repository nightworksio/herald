use herald_core::SecretRef;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::lock::{LockLease, LockPatience};

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
    /// The secret holding the key every record is sealed with, such as
    /// `HERALD_STORE_KEY`: whoever can write the repository could otherwise
    /// forge records herald trusts (ADR-0005).
    pub key: SecretRef,
    /// How long the store's lock holds before it is taken as abandoned.
    #[serde(default)]
    pub lock_lease: LockLease,
    /// How many times a lock with no time to judge its age by is found
    /// held, unchanged, before it is taken as abandoned.
    #[serde(default)]
    pub lock_patience: LockPatience,
}
