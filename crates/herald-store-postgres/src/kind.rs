use std::sync::Arc;

use herald_core::SecretRef;
use herald_kinds::{Local, Opened, Opening, SqlAt, StoreConfig, StoreKind, unopened};
use herald_ports::Secret;
use herald_records::{Dialect, SqlRecords};

use crate::StoreOptions;

/// Postgres, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostgresKind {}

impl StoreKind for PostgresKind {
    const NAME: &str = "postgres";
    const DESCRIPTION: &str = "A Postgres database.";

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

    fn open<'s, 'o: 's>(
        &'s self,
        opening: Arc<dyn Opening + 'o>,
        seal: Option<Secret>,
    ) -> Local<'s, Result<Opened<'o>, String>> {
        Box::pin(async move {
            let url = opening
                .secret(
                    self.url.secret.as_str(),
                    "the postgres store reads its connection URL from it",
                )
                .await?;
            let at = SqlAt::Postgres {
                url: &url,
                connect: self.connect_timeout(),
                statement: self.statement_timeout(),
            };
            let opened = |error| unopened(PostgresKind::NAME, error);
            let sql = opening.sql(at).await.map_err(opened)?;
            let store = SqlRecords::kept(sql, Dialect::Postgres, seal)
                .await
                .map_err(|error| opened(error.to_string()))?;
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
    }
}
