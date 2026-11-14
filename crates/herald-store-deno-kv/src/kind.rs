use std::sync::Arc;

use herald_core::SecretRef;
use herald_kinds::{KvAt, Local, Offer, Opened, Opening, StoreConfig, StoreKind, unopened};
use herald_ports::RecordOptions;
use herald_records::KvRecords;

use crate::StoreOptions;

/// Deno KV, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenoKvKind {}

impl StoreKind for DenoKvKind {
    const NAME: &str = "deno-kv";
    const DESCRIPTION: &str = "Deno KV, on Deno Deploy or wherever a database is opened.";
    const NEEDS: Offer = Offer::DenoKv;

    type Options = StoreOptions;
}

impl StoreConfig for StoreOptions {
    fn kind(&self) -> &'static str {
        DenoKvKind::NAME
    }

    fn key(&self) -> Option<&SecretRef> {
        self.key.as_ref()
    }

    fn open<'s, 'o: 's>(
        &'s self,
        opening: Arc<dyn Opening + 'o>,
        options: RecordOptions,
    ) -> Local<'s, Result<Opened<'o>, String>> {
        Box::pin(async move {
            let kv = opening
                .kv(KvAt::DenoKv(self.path.as_deref()))
                .await
                .map_err(|error| unopened(DenoKvKind::NAME, error))?;
            Ok(Opened::Kept(Box::new(KvRecords::kept(
                kv,
                self.prefix.as_deref(),
                options,
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
    fn its_options_are_a_deno_kv_store() {
        let options = StoreOptions {
            key: Some(secret("K")),
            ..StoreOptions::default()
        };
        assert_eq!(options.kind(), "deno-kv");
        assert_eq!(DenoKvKind::NAME, "deno-kv");
        assert_eq!(options.key(), Some(&secret("K")));
        assert_eq!(options.named(), Vec::<&str>::new());
        assert_eq!(options.note(), None);
        assert!(options.keeps(RecordKind::Notes));
        assert_eq!(options.kept_by(RecordKind::Notes), None);
        assert!(!options.reads_posts());
    }
}
