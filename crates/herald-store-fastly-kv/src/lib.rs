//! herald's records kept one value per record in a Fastly KV Store
//! (ADR-0005), for Fastly Compute.
//!
//! Each write names the generation it replaces, and the KV Store's
//! generation match writes the value only where it still has that one:
//! that is the compare-and-set.

mod kind;

pub use kind::FastlyKvKind;

use herald_core::{Duration, SecretRef};
use jiff::SignedDuration;
use schemars::JsonSchema;
use serde::Deserialize;

/// The KV Store opened where the options name none.
pub const STORE: &str = "herald";

/// The fastly-kv store's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoreOptions {
    /// The KV Store the service is linked to, by name; `herald` where
    /// absent.
    pub store: Option<String>,
    /// The key every record is kept under, its parts split at `/`;
    /// `herald` where absent.
    pub prefix: Option<String>,
    /// The secret holding a key to seal every record with, such as
    /// `HERALD_STORE_KEY`, so a record herald did not seal reads as corrupt
    /// (ADR-0005); records are not sealed where absent.
    pub key: Option<SecretRef>,
    /// How long a deleted record's mark is kept before Fastly expires it,
    /// so a write that read the record before it was deleted fails; "1h"
    /// where absent.
    pub tombstone: Option<Duration>,
}

impl StoreOptions {
    /// How long a deleted record's mark is kept where the options set none.
    pub const TOMBSTONE: SignedDuration = SignedDuration::from_hours(1);

    /// How long a deleted record's mark is kept.
    #[must_use]
    pub fn tombstone(&self) -> SignedDuration {
        self.tombstone
            .map_or(Self::TOMBSTONE, |Duration(kept)| kept)
    }

    /// The KV Store opened.
    #[must_use]
    pub fn store(&self) -> &str {
        self.store.as_deref().unwrap_or(STORE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_store_is_the_one_named_or_else_herald() {
        assert_eq!(StoreOptions::default().store(), "herald");
        let named: StoreOptions =
            serde_json::from_str(r#"{"store": "records", "prefix": "a/b"}"#).unwrap();
        assert_eq!(named.store(), "records");
        assert_eq!(named.prefix.as_deref(), Some("a/b"));
    }

    #[test]
    fn a_deleted_record_s_mark_is_kept_an_hour_or_as_long_as_the_options_say() {
        assert_eq!(
            StoreOptions::default().tombstone(),
            SignedDuration::from_hours(1)
        );
        let set: StoreOptions = serde_json::from_str(r#"{"tombstone": "2d"}"#).unwrap();
        assert_eq!(set.tombstone(), SignedDuration::from_hours(48));
    }
}
