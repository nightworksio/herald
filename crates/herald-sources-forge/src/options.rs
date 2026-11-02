use herald_core::SecretRef;

/// A forge source's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceOptions {
    /// The repository, as `owner/name`, or a GitLab project's full path,
    /// such as `group/subgroup/name`.
    pub repository: String,
    /// The API's base URL, for a forge that is not GitHub, GitLab.com or
    /// Codeberg, such as `https://forge.example/api/v1`. Where absent, it is
    /// the CI run's own when announcing the run's repository, and otherwise
    /// the forge's public instance.
    pub api: Option<String>,
    /// A token that reaches a private repository or raises the API's rate
    /// limit, such as the run's `GITHUB_TOKEN`.
    pub token: Option<SecretRef>,
    /// How many of the newest releases the first poll announces; none where
    /// absent (ADR-0003).
    #[serde(default)]
    pub backfill: usize,
}
