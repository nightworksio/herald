//! herald's records kept one value per record in Deno KV (ADR-0005), on
//! Deno Deploy or wherever a Deno KV database is opened.
//!
//! Each write names the versionstamp it replaces, and Deno KV's atomic
//! check of versionstamps sets the value only where it still has that one:
//! that is the compare-and-set.

use herald_ports::Kv;
use herald_records::{Kept, KvRecords};
use schemars::JsonSchema;
use serde::Deserialize;

/// The key every record is kept under where the options name none.
pub const PREFIX: &str = "herald";

/// The deno-kv store's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoreOptions {
    /// The database `Deno.openKv` opens: a path or a URL; the deployment's
    /// own where absent.
    pub path: Option<String>,
    /// The key every record is kept under, its parts split at `/`;
    /// `herald` where absent.
    pub prefix: Option<String>,
}

impl StoreOptions {
    /// The key every record is kept under.
    #[must_use]
    pub fn prefix(&self) -> &str {
        self.prefix.as_deref().unwrap_or(PREFIX)
    }
}

/// The store a Deno KV database keeps.
pub type DenoKvStore<K> = Kept<KvRecords<K>>;

/// The store kept in `kv`, a Deno KV database, as `options` say.
#[must_use]
pub fn store<K: Kv>(kv: K, options: &StoreOptions) -> DenoKvStore<K> {
    Kept::new(KvRecords::new(kv, options.prefix()))
}

#[cfg(test)]
mod tests {
    use herald_ports::Store as _;
    use herald_testing::{FakeKv, store_conformance};

    use super::*;

    #[test]
    fn the_prefix_is_the_one_named_or_else_herald() {
        assert_eq!(StoreOptions::default().prefix(), "herald");
        let named: StoreOptions =
            serde_json::from_str(r#"{"path": "https://kv.example", "prefix": "apps/herald"}"#)
                .unwrap();
        assert_eq!(named.prefix(), "apps/herald");
        assert_eq!(named.path.as_deref(), Some("https://kv.example"));
    }

    #[test]
    fn the_store_keeps_every_record_under_its_prefix() {
        let kept = store(FakeKv::default(), &StoreOptions::default());
        assert!(kept.capabilities().cursors);
        assert_eq!(pollster::block_on(store_conformance(&kept)), Ok(()));
    }
}
