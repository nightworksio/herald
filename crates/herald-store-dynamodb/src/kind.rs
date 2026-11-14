use std::sync::Arc;

use herald_core::SecretRef;
use herald_kinds::{Local, Opened, Opening, StoreConfig, StoreKind};
use herald_ports::Erased;
use herald_ports::RecordOptions;

use crate::{Dynamodb, StoreOptions};

/// DynamoDB, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynamodbKind {}

impl StoreKind for DynamodbKind {
    const NAME: &str = "dynamodb";
    const DESCRIPTION: &str = "An AWS DynamoDB table.";

    type Options = StoreOptions;
}

impl StoreConfig for StoreOptions {
    fn kind(&self) -> &'static str {
        DynamodbKind::NAME
    }

    fn key(&self) -> Option<&SecretRef> {
        self.key.as_ref()
    }

    fn named(&self) -> Vec<&str> {
        self.secrets().to_vec()
    }

    fn open<'s, 'o: 's>(
        &'s self,
        opening: Arc<dyn Opening + 'o>,
        options: RecordOptions,
    ) -> Local<'s, Result<Opened<'o>, String>> {
        Box::pin(async move {
            let table = Dynamodb::reached(self, &Erased(opening.secrets())).await?;
            let http = opening.connect(table.api())?;
            Ok(Opened::Kept(Box::new(
                table
                    .store(http, opening.clock(), opening.nonce())
                    .with(options),
            )))
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
    fn its_options_are_a_dynamodb_store() {
        let options = StoreOptions {
            key: Some(secret("K")),
            ..StoreOptions::default()
        };
        assert_eq!(options.kind(), "dynamodb");
        assert_eq!(DynamodbKind::NAME, "dynamodb");
        assert_eq!(options.key(), Some(&secret("K")));
        assert_eq!(
            options.named(),
            vec![
                "AWS_ACCESS_KEY_ID",
                "AWS_SECRET_ACCESS_KEY",
                "AWS_SESSION_TOKEN"
            ]
        );
        assert_eq!(options.note(), None);
        assert!(options.keeps(RecordKind::Notes));
        assert_eq!(options.kept_by(RecordKind::Notes), None);
        assert!(!options.reads_posts());
    }
}
