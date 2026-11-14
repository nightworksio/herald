//! herald's records kept one row per record in the SQLite storage of a
//! Cloudflare Durable Object (ADR-0005), so a Worker needs no D1 database.
//!
//! The object runs one request at a time, and each write names the version
//! it replaces in a transaction of the object's storage: that is the
//! compare-and-set.

mod kind;

pub use kind::DurableObjectKind;

use herald_core::SecretRef;
use schemars::JsonSchema;
use serde::Deserialize;

/// The binding a Worker reaches herald's Durable Object through where the
/// options name none.
pub const BINDING: &str = "HERALD";
/// The object the records are kept in where the options name none.
pub(crate) const OBJECT: &str = "herald";

/// The durable-object store's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoreOptions {
    /// The Worker's binding to herald's Durable Object namespace, as
    /// `wrangler.toml` names it; `HERALD` where absent.
    pub binding: Option<String>,
    /// The name of the object that keeps the records; `herald` where
    /// absent.
    pub object: Option<String>,
    /// The secret holding a key to seal every record with, such as
    /// `HERALD_STORE_KEY`, so a record herald did not seal reads as corrupt
    /// (ADR-0005); records are not sealed where absent.
    pub key: Option<SecretRef>,
}

impl StoreOptions {
    /// The binding the namespace is reached through.
    #[must_use]
    pub fn binding(&self) -> &str {
        self.binding.as_deref().unwrap_or(BINDING)
    }

    /// The object that keeps the records.
    #[must_use]
    pub fn object(&self) -> &str {
        self.object.as_deref().unwrap_or(OBJECT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_binding_and_object_are_the_ones_named_or_else_herald_s() {
        let defaults = StoreOptions::default();
        assert_eq!(
            (defaults.binding(), defaults.object()),
            ("HERALD", "herald")
        );
        let named: StoreOptions =
            serde_json::from_str(r#"{"binding": "NS", "object": "records"}"#).unwrap();
        assert_eq!((named.binding(), named.object()), ("NS", "records"));
    }
}
