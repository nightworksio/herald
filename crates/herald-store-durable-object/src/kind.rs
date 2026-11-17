use std::sync::Arc;

use herald_core::SecretRef;
use herald_kinds::{Local, Offer, Opened, Opening, SqlAt, StoreConfig, StoreKind, unopened};
use herald_ports::RecordOptions;
use herald_records::{Dialect, SqlRecords};

use crate::StoreOptions;

/// A Cloudflare Durable Object's storage, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableObjectKind {}

impl StoreKind for DurableObjectKind {
    const NAME: &str = "durable-object";
    const DESCRIPTION: &str =
        "The storage of herald's Cloudflare Durable Object, through the\nWorker's binding.";
    const NEEDS: Offer = Offer::DurableObject;

    type Options = StoreOptions;
}

/// The most bytes a Durable Object's SQLite storage keeps in one row: 2 MB.
const MOST_RECORD: usize = 2_000_000;

impl StoreConfig for StoreOptions {
    fn kind(&self) -> &'static str {
        DurableObjectKind::NAME
    }

    fn key(&self) -> Option<&SecretRef> {
        self.key.as_ref()
    }

    fn most_record(&self) -> Option<usize> {
        Some(MOST_RECORD)
    }

    fn open<'s, 'o: 's>(
        &'s self,
        opening: Arc<dyn Opening + 'o>,
        options: RecordOptions,
    ) -> Local<'s, Result<Opened<'o>, String>> {
        Box::pin(async move {
            let at = SqlAt::DurableObject {
                binding: self.binding(),
                object: self.object(),
            };
            let opened = |error| unopened(DurableObjectKind::NAME, error);
            let sql = opening.sql(at).await.map_err(opened)?;
            let store = SqlRecords::kept(sql, Dialect::Sqlite, options)
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
    fn its_options_are_a_durable_object_store() {
        let options = StoreOptions {
            key: Some(secret("K")),
            ..StoreOptions::default()
        };
        assert_eq!(options.kind(), "durable-object");
        assert_eq!(DurableObjectKind::NAME, "durable-object");
        assert_eq!(options.key(), Some(&secret("K")));
        assert_eq!(options.named(), Vec::<&str>::new());
        assert_eq!(options.note(), None);
        assert!(options.keeps(RecordKind::Notes));
        assert_eq!(options.kept_by(RecordKind::Notes), None);
        assert!(!options.reads_posts());
        assert_eq!(options.most_record(), Some(MOST_RECORD));
    }
}
