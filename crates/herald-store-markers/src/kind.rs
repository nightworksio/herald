use std::any::Any;
use std::sync::Arc;

use herald_core::{RecordKind, SecretRef};
use herald_kinds::{Local, Opened, Opening, StoreConfig, StoreKind, same};
use herald_ports::Secret;
use schemars::JsonSchema;
use serde::Deserialize;

/// The markers store's options in `herald.toml`: none beside its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename = "Options")]
pub struct MarkersOptions {}

/// The destinations' own posts, as a kind of store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkersKind {}

impl StoreKind for MarkersKind {
    const NAME: &'static str = "markers";
    const DESCRIPTION: &'static str = "The destinations' own posts.";
    const RETAINED: bool = false;

    type Options = MarkersOptions;
}

impl StoreConfig for MarkersOptions {
    fn kind(&self) -> &'static str {
        MarkersKind::NAME
    }

    fn key(&self) -> Option<&SecretRef> {
        None
    }

    fn keeps(&self, _: RecordKind) -> bool {
        false
    }

    fn reads_posts(&self) -> bool {
        true
    }

    fn open<'s, 'o: 's>(
        &'s self,
        _: Arc<dyn Opening + 'o>,
        _: Option<Secret>,
    ) -> Local<'s, Result<Opened<'o>, String>> {
        Box::pin(std::future::ready(Ok(Opened::Posts)))
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

    #[test]
    fn its_options_are_a_markers_store() {
        let options = MarkersOptions {};
        assert_eq!(options.kind(), "markers");
        assert_eq!(MarkersKind::NAME, "markers");
        assert_eq!(options.key(), None);
        assert_eq!(options.named(), Vec::<&str>::new());
        assert_eq!(options.note(), None);
        assert!(!options.keeps(RecordKind::Pins));
        assert_eq!(options.kept_by(RecordKind::Pins), None);
        assert!(options.reads_posts());
        assert!(options.same(options.clone().as_any()));
        assert!(!options.same(&0_u8));
    }
}
