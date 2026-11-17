//! How an owner source finds the repositories that join or leave it.

use super::SourceOptions;

/// How an owner source keeps its membership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Discovery {
    /// The owner listed each round, conditionally, and again at once where
    /// a delivery says its repositories changed.
    #[default]
    List,
    /// The owner listed only where the source holds no listing, its
    /// selection changed, or a delivery said its repositories changed.
    Push,
}

impl SourceOptions {
    /// How an owner source keeps its membership.
    #[must_use]
    pub fn discovery(&self) -> Discovery {
        self.discovery.unwrap_or_default()
    }
}
