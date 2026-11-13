use std::sync::Arc;

use herald_core::SecretRef;
use herald_kinds::{Local, Offer, Opened, Opening, SqlAt, StoreConfig, StoreKind, unopened};
use herald_ports::Secret;
use herald_records::Seal;

use crate::StoreOptions;

/// Cloudflare D1, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum D1Kind {}

impl StoreKind for D1Kind {
    const NAME: &str = "d1";
    const DESCRIPTION: &str = "Cloudflare D1, through the Worker's binding.";
    const NEEDS: Offer = Offer::D1;

    type Options = StoreOptions;
}

impl StoreConfig for StoreOptions {
    fn kind(&self) -> &'static str {
        D1Kind::NAME
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
            let opened = |error| unopened(D1Kind::NAME, error);
            let sql = opening
                .sql(SqlAt::D1(self.binding()))
                .await
                .map_err(opened)?;
            let store = crate::store(sql)
                .await
                .map_err(|error| opened(error.to_string()))?;
            Ok(Opened::Kept(Box::new(store.sealed(seal.map(Seal::new)))))
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
    fn its_options_are_a_d1_store() {
        let options = StoreOptions {
            key: Some(secret("K")),
            ..StoreOptions::default()
        };
        assert_eq!(options.kind(), "d1");
        assert_eq!(D1Kind::NAME, "d1");
        assert_eq!(options.key(), Some(&secret("K")));
        assert_eq!(options.named(), Vec::<&str>::new());
        assert_eq!(options.note(), None);
        assert!(options.keeps(RecordKind::Notes));
        assert_eq!(options.kept_by(RecordKind::Notes), None);
        assert!(!options.reads_posts());
    }
}
