//! herald's records kept one row per record in a SQLite file (ADR-0005),
//! on a volume or a disk that outlives the run.
//!
//! Each write names the version it replaces, and a transaction holding the
//! file's write lock changes the row only where it still has that version:
//! that is the compare-and-set.

use std::path::{Path, PathBuf};

use herald_core::Duration;
use herald_ports::{Sql, StoreError};
use herald_records::{Dialect, Kept, SqlRecords};
use jiff::SignedDuration;
use schemars::JsonSchema;
use serde::Deserialize;

/// The file a store keeps its records in where its options name none, beside
/// `herald.toml`.
pub const FILE: &str = "herald.sqlite";

/// How long a transaction waits for another process to let go of the file
/// where the options name no time.
pub const BUSY: SignedDuration = SignedDuration::from_secs(5);

/// The sqlite store's options in `herald.toml` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoreOptions {
    /// The database file, relative to `herald.toml`; `herald.sqlite` beside
    /// it where absent.
    pub path: Option<String>,
    /// How long a transaction waits for another process to let go of the
    /// file; 5s where absent.
    pub busy: Option<Duration>,
}

impl StoreOptions {
    /// The database file, for a config in the directory `directory`.
    #[must_use]
    pub fn file(&self, directory: &Path) -> PathBuf {
        directory.join(self.path.as_deref().unwrap_or(FILE))
    }

    /// How long a transaction waits for another process to let go of the
    /// file.
    #[must_use]
    pub fn busy(&self) -> SignedDuration {
        self.busy.map_or(BUSY, |Duration(busy)| busy)
    }
}

/// The store a SQLite database keeps.
pub type SqliteStore<S> = Kept<SqlRecords<S>>;

/// The store kept in `sql`, a SQLite database, with its tables made where
/// they are missing.
///
/// # Errors
///
/// When the database cannot be reached, or a newer herald laid it out.
pub async fn store<S: Sql>(sql: S) -> Result<SqliteStore<S>, StoreError> {
    SqlRecords::open(sql, Dialect::Sqlite).await.map(Kept::new)
}

#[cfg(test)]
mod tests {
    use herald_ports::{Changed, SqlError, Store as _, Value};
    use herald_testing::ScriptedSql;

    use super::*;

    #[test]
    fn the_file_is_the_one_named_beside_the_config_or_else_herald_sqlite() {
        let directory = Path::new("/srv/herald");
        assert_eq!(
            StoreOptions::default().file(directory),
            Path::new("/srv/herald/herald.sqlite")
        );
        let named = StoreOptions {
            path: Some("state/records.db".to_owned()),
            busy: None,
        };
        assert_eq!(
            named.file(directory),
            Path::new("/srv/herald/state/records.db")
        );
    }

    #[test]
    fn a_transaction_waits_the_time_named_or_else_five_seconds() {
        assert_eq!(StoreOptions::default().busy(), BUSY);
        let named = StoreOptions {
            path: None,
            busy: Some(Duration(SignedDuration::from_secs(30))),
        };
        assert_eq!(named.busy(), SignedDuration::from_secs(30));
    }

    #[test]
    fn a_database_that_cannot_be_reached_opens_no_store() {
        let unreachable = ScriptedSql::default().answering(Err(SqlError::Unavailable {
            reason: "locked".to_owned(),
        }));
        assert_eq!(
            pollster::block_on(store(unreachable)).err(),
            Some(StoreError::Unavailable {
                reason: "the database refused the statements: locked".to_owned()
            })
        );
        let laid_out = Changed {
            rows: vec![vec![Value::Integer(1)]],
            changed: 0,
        };
        let opened = ScriptedSql::default().answering(Ok(vec![
            Changed::default(),
            Changed::default(),
            Changed::default(),
            laid_out,
        ]));
        let kept = pollster::block_on(store(opened)).unwrap();
        assert!(kept.capabilities().cursors);
    }
}
