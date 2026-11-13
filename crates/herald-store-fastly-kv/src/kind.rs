use std::sync::Arc;

use herald_core::SecretRef;
use herald_kinds::{KvAt, Local, Opened, Opening, StoreConfig, StoreKind, unopened};
use herald_ports::Secret;
use herald_records::KvRecords;

use crate::StoreOptions;

/// A Fastly KV Store, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FastlyKvKind {}

impl StoreKind for FastlyKvKind {
    const NAME: &str = "fastly-kv";
    const DESCRIPTION: &str = "A Fastly KV Store, on Fastly Compute.";

    type Options = StoreOptions;
}

impl StoreConfig for StoreOptions {
    fn kind(&self) -> &'static str {
        FastlyKvKind::NAME
    }

    fn key(&self) -> Option<&SecretRef> {
        self.key.as_ref()
    }

    fn open<'s, 'o: 's>(
        &'s self,
        opening: Arc<dyn Opening + 'o>,
        seal: Option<Secret>,
    ) -> Local<'s, Result<Opened<'o>, String>> {
        Box::pin(async move {
            let kv = opening
                .kv(KvAt::FastlyKv(self.store()))
                .await
                .map_err(|error| unopened(FastlyKvKind::NAME, error))?;
            Ok(Opened::Kept(Box::new(KvRecords::kept(
                kv,
                self.prefix.as_deref(),
                seal,
            ))))
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
    fn its_options_are_a_fastly_kv_store() {
        let options = StoreOptions {
            key: Some(secret("K")),
            ..StoreOptions::default()
        };
        assert_eq!(options.kind(), "fastly-kv");
        assert_eq!(FastlyKvKind::NAME, "fastly-kv");
        assert_eq!(options.key(), Some(&secret("K")));
        assert_eq!(options.named(), Vec::<&str>::new());
        assert_eq!(options.note(), None);
        assert!(options.keeps(RecordKind::Notes));
        assert_eq!(options.kept_by(RecordKind::Notes), None);
        assert!(!options.reads_posts());
    }
}
