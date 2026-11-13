//! herald's records kept one row per record in a SQLite file (ADR-0005),
//! on a volume or a disk that outlives the run.
//!
//! Each write names the version it replaces, and a transaction holding the
//! file's write lock changes the row only where it still has that version:
//! that is the compare-and-set.

mod kind;

pub use kind::SqliteKind;

use std::path::{Path, PathBuf};

use herald_core::{Duration, SecretRef};
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
    /// The secret holding a key to seal every record with, such as
    /// `HERALD_STORE_KEY`, so a record herald did not seal reads as corrupt
    /// (ADR-0005); records are not sealed where absent.
    pub key: Option<SecretRef>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_is_the_one_named_beside_the_config_or_else_herald_sqlite() {
        let directory = Path::new("/srv/herald");
        assert_eq!(
            StoreOptions::default().file(directory),
            Path::new("/srv/herald/herald.sqlite")
        );
        let named = StoreOptions {
            key: None,
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
            key: None,
            path: None,
            busy: Some(Duration(SignedDuration::from_secs(30))),
        };
        assert_eq!(named.busy(), SignedDuration::from_secs(30));
    }
}
