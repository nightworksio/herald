//! Where the teams a view's load counts come from (ADR-0075, ADR-0077).

use super::SourceOptions;

/// Where the teams a view's load counts come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TeamsFrom {
    /// The config's `[teams.<name>]` alone.
    #[default]
    Config,
    /// The config's, and the forge's own teams of the organization or group
    /// owning the source's repositories.
    Forge,
}

impl SourceOptions {
    /// Whether a view showing its load reads the forge's own teams.
    #[must_use]
    pub fn forge_teams(&self) -> bool {
        self.teams.unwrap_or_default() == TeamsFrom::Forge
    }
}
