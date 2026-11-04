//! herald's records kept one row per record in a Postgres database
//! (ADR-0005), for the CLI, the Lambda and the container.
//!
//! Each write names the version it replaces, and a transaction changes the
//! row only where it still has that version, which a concurrent writer's
//! row lock makes it wait to see: that is the compare-and-set.

use herald_core::SecretRef;
use herald_ports::{Sql, StoreError};
use herald_records::{Dialect, Kept, SqlRecords};
use schemars::JsonSchema;
use serde::Deserialize;

/// The postgres store's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoreOptions {
    /// The secret holding the database's connection URL, such as
    /// `postgres://herald:…@db.example/herald`. herald reaches it over TLS
    /// unless the URL sets `sslmode=disable`.
    pub url: SecretRef,
}

/// The store a Postgres database keeps.
pub type PostgresStore<S> = Kept<SqlRecords<S>>;

/// The store kept in `sql`, a Postgres database, with its tables made where
/// they are missing.
///
/// # Errors
///
/// When the database cannot be reached, or a newer herald laid it out.
pub async fn store<S: Sql>(sql: S) -> Result<PostgresStore<S>, StoreError> {
    SqlRecords::open(sql, Dialect::Postgres)
        .await
        .map(Kept::new)
}

#[cfg(test)]
mod tests {
    use herald_ports::{Changed, SqlError, Store as _, Value};
    use herald_testing::ScriptedSql;

    use super::*;

    #[test]
    fn a_database_that_cannot_be_reached_opens_no_store() {
        let unreachable = ScriptedSql::default().answering(Err(SqlError::Unavailable {
            reason: "refused".to_owned(),
        }));
        assert_eq!(
            pollster::block_on(store(unreachable)).err(),
            Some(StoreError::Unavailable {
                reason: "the database refused the statements: refused".to_owned()
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
