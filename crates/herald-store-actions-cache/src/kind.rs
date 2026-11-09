use std::any::Any;

use herald_core::{RecordKind, SecretRef};
use herald_kinds::{StoreConfig, StoreKind, same};

use crate::StoreOptions;

/// What the Actions cache keeps apart and drops (ADR-0005).
const SCOPED: &str = "The Actions cache keeps records apart for each ref a run is on, and drops what no run reads for a week: a release run on its tag does not see what a run on the default branch wrote. Run housekeeping and polling on one ref, or choose a store that keeps records across refs.";

/// The GitHub Actions cache, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionsCacheKind {}

impl StoreKind for ActionsCacheKind {
    const NAME: &'static str = "actions-cache";
    const DESCRIPTION: &'static str = "Generations of the GitHub Actions cache.";

    type Options = StoreOptions;
}

impl StoreConfig for StoreOptions {
    fn kind(&self) -> &'static str {
        ActionsCacheKind::NAME
    }

    fn key(&self) -> Option<&SecretRef> {
        Some(&self.key)
    }

    fn keeps(&self, kind: RecordKind) -> bool {
        self.keeps.contains(&kind)
    }

    fn kept_by(&self, kind: RecordKind) -> Option<String> {
        Some(format!("add \"{}\" to its keeps", kind.name()))
    }

    fn note(&self) -> Option<&'static str> {
        Some(SCOPED)
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
    use super::*;

    fn secret(name: &str) -> SecretRef {
        SecretRef {
            secret: name.to_owned(),
        }
    }

    #[test]
    fn its_options_are_a_actions_cache_store() {
        let options = StoreOptions {
            prefix: None,
            keeps: vec![RecordKind::Pins],
            key: secret("K"),
        };
        assert_eq!(options.kind(), "actions-cache");
        assert_eq!(ActionsCacheKind::NAME, "actions-cache");
        assert_eq!(options.key(), Some(&secret("K")));
        assert_eq!(options.named(), Vec::<&str>::new());
        assert_eq!(options.note(), Some(SCOPED));
        assert!(options.keeps(RecordKind::Pins));
        assert!(!options.keeps(RecordKind::Notes));
        assert_eq!(
            options.kept_by(RecordKind::Notes).as_deref(),
            Some("add \"notes\" to its keeps")
        );
        assert!(!options.reads_posts());
        assert!(options.same(options.clone().as_any()));
        assert!(!options.same(&0_u8));
    }
}
