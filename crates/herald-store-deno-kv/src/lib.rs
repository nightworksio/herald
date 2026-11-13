//! herald's records kept one value per record in Deno KV (ADR-0005), on
//! Deno Deploy or wherever a Deno KV database is opened.
//!
//! Each write names the versionstamp it replaces, and Deno KV's atomic
//! check of versionstamps sets the value only where it still has that one:
//! that is the compare-and-set.

mod kind;

pub use kind::DenoKvKind;

use herald_core::SecretRef;
use schemars::JsonSchema;
use serde::Deserialize;

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
    /// The secret holding a key to seal every record with, such as
    /// `HERALD_STORE_KEY`, so a record herald did not seal reads as corrupt
    /// (ADR-0005); records are not sealed where absent.
    pub key: Option<SecretRef>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_options_name_the_database_and_the_prefix() {
        assert_eq!(StoreOptions::default().prefix, None);
        let named: StoreOptions =
            serde_json::from_str(r#"{"path": "https://kv.example", "prefix": "apps/herald"}"#)
                .unwrap();
        assert_eq!(named.prefix.as_deref(), Some("apps/herald"));
        assert_eq!(named.path.as_deref(), Some("https://kv.example"));
    }
}
