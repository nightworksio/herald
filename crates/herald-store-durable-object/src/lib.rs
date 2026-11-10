//! herald's records kept one row per record in the SQLite storage of a
//! Cloudflare Durable Object (ADR-0005), so a Worker needs no D1 database.
//!
//! The object runs one request at a time, and each write names the version
//! it replaces in a transaction of the object's storage: that is the
//! compare-and-set.

mod kind;

pub use kind::DurableObjectKind;

use herald_core::SecretRef;
use herald_ports::{Sql, StoreError};
use herald_records::{Dialect, Kept, SqlRecords};
use schemars::JsonSchema;
use serde::Deserialize;

/// The binding a Worker reaches herald's Durable Object through where the
/// options name none.
pub const BINDING: &str = "HERALD";
/// The object the records are kept in where the options name none.
pub const OBJECT: &str = "herald";

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

/// The store a Durable Object's storage keeps.
pub type DurableObjectStore<S> = Kept<SqlRecords<S>>;

/// The store kept in `sql`, a Durable Object's storage, with its tables
/// made where they are missing.
///
/// # Errors
///
/// When the storage refuses the statements, or a newer herald laid it out.
pub async fn store<S: Sql>(sql: S) -> Result<DurableObjectStore<S>, StoreError> {
    SqlRecords::open(sql, Dialect::Sqlite).await.map(Kept::new)
}

#[cfg(test)]
mod tests {
    use herald_ports::{Changed, SqlError, Store as _, Value};
    use herald_testing::ScriptedSql;

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

    #[test]
    fn a_database_that_cannot_be_reached_opens_no_store_and_one_that_can_does() {
        let unreachable = ScriptedSql::default().answering(Err(SqlError::Unavailable {
            reason: "down".to_owned(),
        }));
        assert_eq!(
            pollster::block_on(store(unreachable)).err(),
            Some(StoreError::Unavailable {
                reason: "the database refused the statements: down".to_owned()
            })
        );
        let opened = ScriptedSql::default().answering(Ok(vec![
            Changed::default(),
            Changed::default(),
            Changed::default(),
            Changed {
                rows: vec![vec![Value::Integer(herald_records::LAYOUT)]],
                changed: 0,
            },
        ]));
        assert!(
            pollster::block_on(store(opened))
                .unwrap()
                .cursors()
                .is_some()
        );
    }
}
