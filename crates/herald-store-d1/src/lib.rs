//! herald's records kept one row per record in Cloudflare D1 (ADR-0005),
//! reached through the Worker's binding.
//!
//! Each write names the version it replaces, and a batch D1 runs as one
//! transaction changes the row only where it still has that version: that
//! is the compare-and-set.

use herald_ports::{Sql, StoreError};
use herald_records::{Dialect, Kept, SqlRecords};
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
}

impl StoreOptions {
    /// The binding the database is reached through.
    #[must_use]
    pub fn binding(&self) -> &str {
        self.binding.as_deref().unwrap_or(BINDING)
    }
}

/// The store a D1 database keeps.
pub type D1Store<S> = Kept<SqlRecords<S>>;

/// The store kept in `sql`, a D1 database, with its tables made where they
/// are missing.
///
/// # Errors
///
/// When the database cannot be reached, or a newer herald laid it out.
pub async fn store<S: Sql>(sql: S) -> Result<D1Store<S>, StoreError> {
    SqlRecords::open(sql, Dialect::Sqlite).await.map(Kept::new)
}

#[cfg(test)]
mod tests {
    use herald_ports::{Changed, SqlError, Store as _, Value};
    use herald_testing::ScriptedSql;

    use super::*;

    #[test]
    fn the_binding_is_the_one_named_or_else_herald_db() {
        assert_eq!(StoreOptions::default().binding(), "HERALD_DB");
        let named: StoreOptions = serde_json::from_str(r#"{"binding": "DB"}"#).unwrap();
        assert_eq!(named.binding(), "DB");
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
                rows: vec![vec![Value::Integer(1)]],
                changed: 0,
            },
        ]));
        assert!(
            pollster::block_on(store(opened))
                .unwrap()
                .capabilities()
                .cursors
        );
    }
}
