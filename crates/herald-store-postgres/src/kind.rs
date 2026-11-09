use std::any::Any;

use herald_core::SecretRef;
use herald_kinds::{StoreConfig, StoreKind, same};

use crate::StoreOptions;

/// Postgres, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostgresKind {}

impl StoreKind for PostgresKind {
    const NAME: &'static str = "postgres";
    const DESCRIPTION: &'static str = "A Postgres database.";

    type Options = StoreOptions;
}

impl StoreConfig for StoreOptions {
    fn kind(&self) -> &'static str {
        PostgresKind::NAME
    }

    fn key(&self) -> Option<&SecretRef> {
        self.key.as_ref()
    }

    fn named(&self) -> Vec<&str> {
        vec![self.url.secret.as_str()]
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
    fn its_options_are_a_postgres_store() {
        let options = StoreOptions {
            url: secret("URL"),
            connect_timeout: None,
            statement_timeout: None,
            key: Some(secret("K")),
        };
        assert_eq!(options.kind(), "postgres");
        assert_eq!(PostgresKind::NAME, "postgres");
        assert_eq!(options.key(), Some(&secret("K")));
        assert_eq!(options.named(), vec!["URL"]);
        assert_eq!(options.note(), None);
        assert!(options.keeps(RecordKind::Notes));
        assert_eq!(options.kept_by(RecordKind::Notes), None);
        assert!(!options.reads_posts());
        assert!(options.same(options.clone().as_any()));
        assert!(!options.same(&0_u8));
    }
}
