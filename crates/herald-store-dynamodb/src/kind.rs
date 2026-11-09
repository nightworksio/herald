use std::any::Any;

use herald_core::SecretRef;
use herald_kinds::{StoreConfig, StoreKind, same};

use crate::StoreOptions;

/// DynamoDB, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynamodbKind {}

impl StoreKind for DynamodbKind {
    const NAME: &'static str = "dynamodb";
    const DESCRIPTION: &'static str = "An AWS DynamoDB table.";

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
        assert!(options.same(options.clone().as_any()));
        assert!(!options.same(&0_u8));
    }
}
