use std::num::NonZeroUsize;

use herald_core::Size;
use herald_wire::{PARSE_LIMIT, Reading};

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
    /// How many of the newest entries one poll reads; 30 where absent.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub page: Option<NonZeroUsize>,
    /// How many identities the cursor remembers, newest first; 500 where
    /// absent, and at least `page`.
    #[schemars(with = "Option<NonZeroUsize>")]
    pub remembered: Option<NonZeroUsize>,
    /// The most of an answer parsed; "4MiB" where absent, and at most four
    /// times that.
    pub parse: Option<Size>,
}

impl FeedOptions {
    /// Whether the options are sound.
    ///
    /// # Errors
    ///
    /// When `url` is not a web address.
    pub fn check(&self) -> Result<(), String> {
        if herald_core::web_url(&self.url) {
            self.reading().check(PARSE_LIMIT)
        } else {
            Err(format!(
                "\"{}\" is not a web address; give the feed's https:// URL",
                self.url
            ))
        }
    }

    /// How a poll of the feed reads.
    #[must_use]
    pub fn reading(&self) -> Reading {
        Reading::of(self.page, self.remembered, self.parse, PARSE_LIMIT)
    }
}
