use std::any::Any;

use herald_core::{RecordKind, SecretRef};

use super::*;
use crate::same;

/// Options of a kind that keeps every default.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Plain {
    key: Option<SecretRef>,
}

impl StoreConfig for Plain {
    fn kind(&self) -> &'static str {
        "plain"
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

/// Options of another kind, which names a token beside its key.
#[derive(Debug, PartialEq, Eq)]
struct Tokened;

impl StoreConfig for Tokened {
    fn kind(&self) -> &'static str {
        "tokened"
    }

    fn key(&self) -> Option<&SecretRef> {
        None
    }

    fn named(&self) -> Vec<&str> {
        vec!["TOKEN"]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}

fn sealed() -> Plain {
    Plain {
        key: Some(SecretRef {
            secret: "KEY".to_owned(),
        }),
    }
}

#[test]
fn a_store_keeps_every_kind_itself_and_says_nothing_where_its_kind_keeps_the_defaults() {
    let store = Store::new(sealed());
    for kind in [
        RecordKind::Subscriptions,
        RecordKind::Pins,
        RecordKind::Notes,
        RecordKind::Latest,
        RecordKind::Keys,
    ] {
        assert!(store.keeps(kind));
        assert_eq!(store.kept_by(kind), None);
    }
    assert!(!store.reads_posts());
    assert_eq!(store.note(), None);
    assert_eq!(store.kind(), "plain");
}

#[test]
fn a_store_names_its_secrets_and_then_its_key() {
    assert_eq!(Store::new(sealed()).secrets(), vec!["KEY"]);
    assert_eq!(
        Store::new(Plain { key: None }).secrets(),
        Vec::<&str>::new()
    );
    assert_eq!(Store::new(Tokened).secrets(), vec!["TOKEN"]);
}

#[test]
fn a_store_is_read_as_its_own_kind_and_equal_only_to_the_same_options() {
    let store = Store::new(sealed());
    assert_eq!(store.of::<Plain>(), Some(&sealed()));
    assert_eq!(store.of::<Tokened>(), None);
    assert_eq!(store, store.clone());
    assert_eq!(store, Store::new(sealed()));
    assert_ne!(store, Store::new(Plain { key: None }));
    assert_ne!(store, Store::new(Tokened));
    assert_eq!(format!("{store:?}"), format!("{:?}", sealed()));
}
