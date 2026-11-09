use std::any::Any;

use herald_core::SecretRef;
use herald_kinds::{StoreConfig, StoreKind, same};

use crate::StoreOptions;

/// Where the store must run (ADR-0005).
const FILED: &str = "Every record is kept in one SQLite file, so run herald where that file outlives the run: a CI runner's disk is gone when its job ends.";

/// SQLite, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqliteKind {}

impl StoreKind for SqliteKind {
    const NAME: &'static str = "sqlite";
    const DESCRIPTION: &'static str = "A SQLite file, on a volume or a persistent disk.";

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

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
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
        assert!(options.same(options.clone().as_any()));
        assert!(!options.same(&0_u8));
    }
}
