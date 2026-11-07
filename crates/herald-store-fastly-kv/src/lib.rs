//! herald's records kept one value per record in a Fastly KV Store
//! (ADR-0005), for Fastly Compute.
//!
//! Each write names the generation it replaces, and the KV Store's
//! generation match writes the value only where it still has that one:
//! that is the compare-and-set.

use herald_ports::Kv;
use herald_records::{Kept, KvRecords};
use schemars::JsonSchema;
use serde::Deserialize;

/// The KV Store opened where the options name none.
pub const STORE: &str = "herald";
/// The key every record is kept under where the options name none.
pub const PREFIX: &str = "herald";

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
}

impl StoreOptions {
    /// The KV Store opened.
    #[must_use]
    pub fn store(&self) -> &str {
        self.store.as_deref().unwrap_or(STORE)
    }

    /// The key every record is kept under.
    #[must_use]
    pub fn prefix(&self) -> &str {
        self.prefix.as_deref().unwrap_or(PREFIX)
    }
}

/// The store a Fastly KV Store keeps.
pub type FastlyKvStore<K> = Kept<KvRecords<K>>;

/// The store kept in `kv`, a Fastly KV Store, as `options` say.
#[must_use]
pub fn store<K: Kv>(kv: K, options: &StoreOptions) -> FastlyKvStore<K> {
    Kept::new(KvRecords::new(kv, options.prefix()))
}

#[cfg(test)]
mod tests {
    use herald_ports::Store as _;
    use herald_testing::{FakeKv, store_conformance};

    use super::*;

    #[test]
    fn the_store_and_prefix_are_the_ones_named_or_else_herald() {
        let defaults = StoreOptions::default();
        assert_eq!((defaults.store(), defaults.prefix()), ("herald", "herald"));
        let named: StoreOptions =
            serde_json::from_str(r#"{"store": "records", "prefix": "a/b"}"#).unwrap();
        assert_eq!((named.store(), named.prefix()), ("records", "a/b"));
    }

    #[test]
    fn the_store_keeps_every_record_under_its_prefix() {
        let kept = store(FakeKv::default(), &StoreOptions::default());
        assert!(kept.capabilities().cursors);
        assert_eq!(pollster::block_on(store_conformance(&kept)), Ok(()));
    }
}
