use schemars::JsonSchema;
use serde::Deserialize;

/// The actions-cache store's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoreOptions {
    /// What every generation's key starts with, so two configs in one
    /// repository keep their records apart; `herald-state` where absent.
    pub prefix: Option<String>,
}
