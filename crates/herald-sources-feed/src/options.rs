/// A feed source's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeedOptions {
    /// The feed's URL, RSS 2.0 or Atom 1.0.
    pub url: String,
    /// How many of the newest entries the first poll announces; none where
    /// absent (ADR-0003).
    #[serde(default)]
    pub backfill: usize,
}

impl FeedOptions {
    /// Whether the options are sound.
    ///
    /// # Errors
    ///
    /// When `url` is not a web address.
    pub fn check(&self) -> Result<(), String> {
        if herald_core::web_url(&self.url) {
            Ok(())
        } else {
            Err(format!(
                "\"{}\" is not a web address; give the feed's https:// URL",
                self.url
            ))
        }
    }
}
