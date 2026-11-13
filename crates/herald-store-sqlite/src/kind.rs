use std::path::Path;
use std::sync::Arc;

use herald_core::SecretRef;
use herald_kinds::{Local, Offer, Opened, Opening, SqlAt, StoreConfig, StoreKind};
use herald_ports::Secret;
use herald_records::{Dialect, SqlRecords};

use crate::StoreOptions;

/// Where the store must run (ADR-0005).
const FILED: &str = "Every record is kept in one SQLite file, so run herald where that file outlives the run: a CI runner's disk is gone when its job ends.";

/// SQLite, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqliteKind {}

impl StoreKind for SqliteKind {
    const NAME: &str = "sqlite";
    const DESCRIPTION: &str = "A SQLite file, on a volume or a persistent disk.";
    const NEEDS: Offer = Offer::Sqlite;

    type Options = StoreOptions;
}

impl StoreConfig for StoreOptions {
    fn kind(&self) -> &'static str {
        SqliteKind::NAME
    }

    fn key(&self) -> Option<&SecretRef> {
        self.key.as_ref()
    }

    fn note(&self) -> Option<&'static str> {
        Some(FILED)
    }

    fn open<'s, 'o: 's>(
        &'s self,
        opening: Arc<dyn Opening + 'o>,
        seal: Option<Secret>,
    ) -> Local<'s, Result<Opened<'o>, String>> {
        Box::pin(async move {
            let file = self.file(Path::new(""));
            let filed = |error: String| {
                format!(
                    "the sqlite store {} could not be opened: {error}",
                    file.display()
                )
            };
            let at = SqlAt::Sqlite {
                file: &file,
                busy: self.busy(),
            };
            let sql = opening.sql(at).await.map_err(filed)?;
            let store = SqlRecords::kept(sql, Dialect::Sqlite, seal)
                .await
                .map_err(|error| filed(error.to_string()))?;
            Ok(Opened::Kept(Box::new(store)))
        })
    }
}

#[cfg(test)]
mod tests {
    use herald_core::RecordKind;

    use super::*;

    fn secret(name: &str) -> SecretRef {
        SecretRef {
            secret: name.to_owned(),
        }
    }

    #[test]
    fn its_options_are_a_sqlite_store() {
        let options = StoreOptions {
            key: Some(secret("K")),
            ..StoreOptions::default()
        };
        assert_eq!(options.kind(), "sqlite");
        assert_eq!(SqliteKind::NAME, "sqlite");
        assert_eq!(options.key(), Some(&secret("K")));
        assert_eq!(options.named(), Vec::<&str>::new());
        assert_eq!(options.note(), Some(FILED));
        assert!(options.keeps(RecordKind::Notes));
        assert_eq!(options.kept_by(RecordKind::Notes), None);
        assert!(!options.reads_posts());
    }
}
