//! herald's records kept one row per record in Cloudflare D1 (ADR-0005),
//! reached through the Worker's binding.
//!
//! Each write names the version it replaces, and a batch D1 runs as one
//! transaction changes the row only where it still has that version: that
//! is the compare-and-set.

mod kind;

pub use kind::D1Kind;

use herald_core::SecretRef;
use schemars::JsonSchema;
use serde::Deserialize;

/// The binding a Worker reaches its D1 database through where the options
/// name none.
pub const BINDING: &str = "HERALD_DB";

/// The d1 store's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoreOptions {
    /// The Worker's binding to the database, as `wrangler.toml` names it;
    /// `HERALD_DB` where absent.
    pub binding: Option<String>,
    /// The secret holding a key to seal every record with, such as
    /// `HERALD_STORE_KEY`, so a record herald did not seal reads as corrupt
    /// (ADR-0005); records are not sealed where absent.
    pub key: Option<SecretRef>,
}

impl StoreOptions {
    /// The binding the database is reached through.
    #[must_use]
    pub fn binding(&self) -> &str {
        self.binding.as_deref().unwrap_or(BINDING)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_binding_is_the_one_named_or_else_herald_db() {
        assert_eq!(StoreOptions::default().binding(), "HERALD_DB");
        let named: StoreOptions = serde_json::from_str(r#"{"binding": "DB"}"#).unwrap();
        assert_eq!(named.binding(), "DB");
    }
}
