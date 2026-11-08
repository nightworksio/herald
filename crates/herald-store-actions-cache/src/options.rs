use herald_core::RecordKind;
use schemars::JsonSchema;
use serde::Deserialize;

/// The actions-cache store's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
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
}

impl StoreOptions {
    /// Whether it keeps records of `kind`.
    #[must_use]
    pub fn keeps(&self, kind: RecordKind) -> bool {
        self.keeps.contains(&kind)
    }
}
