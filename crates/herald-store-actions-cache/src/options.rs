use herald_core::{RecordKind, SecretRef};
use schemars::JsonSchema;
use serde::Deserialize;

/// The actions-cache store's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoreOptions {
    /// What every generation's key starts with, so two configs in one
    /// repository keep their records apart; `herald-state` where absent.
    pub prefix: Option<String>,
    /// What it keeps besides the ledger, cursors, schedules and deliveries:
    /// any of "subscriptions", "pins", "notes", "latest" and "keys". Each
    /// is rewritten with every other record on every write, and counts
    /// against the cache's quota; none where absent.
    #[serde(default)]
    pub keeps: Vec<RecordKind>,
    /// The secret holding the key every record is sealed with, such as
    /// `HERALD_STORE_KEY`: whoever can write the repository could otherwise
    /// forge records herald trusts (ADR-0005).
    pub key: SecretRef,
}
