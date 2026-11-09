use std::any::Any;

use herald_core::SecretRef;
use herald_kinds::{StoreConfig, StoreKind, same};

use crate::StoreOptions;

/// A Fastly KV Store, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FastlyKvKind {}

impl StoreKind for FastlyKvKind {
    const NAME: &'static str = "fastly-kv";
    const DESCRIPTION: &'static str = "A Fastly KV Store, on Fastly Compute.";

    type Options = StoreOptions;
}

impl StoreConfig for StoreOptions {
    fn kind(&self) -> &'static str {
        FastlyKvKind::NAME
    }

    fn key(&self) -> Option<&SecretRef> {
        self.key.as_ref()
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
        assert!(options.same(options.clone().as_any()));
        assert!(!options.same(&0_u8));
    }
}
