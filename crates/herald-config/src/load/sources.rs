//! `[sources]`: each source read by its kind.

use std::collections::BTreeMap;

use herald_core::SourceName;
use herald_kinds::Source;

use super::{Loader, Value};
use crate::kinds::At;

impl Loader<'_> {
    pub(super) fn sources(&mut self, value: Option<Value<'_>>) -> BTreeMap<SourceName, Source> {
        let mut sources = BTreeMap::new();
        for (name, value) in self.named(value, "sources") {
            let path = format!("sources.{name}");
            let span = value.span();
            let kinds = self.kinds;
            let Some((kind, rest)) = self.kinded(value, &path, &kinds.source_kinds()) else {
                continue;
            };
            let at = At {
                path: &path,
                span: &span,
            };
            let source = kinds
                .source_named(&kind)
                .and_then(|kind| kind.parse(self, rest, at));
            if let Some(source) = source {
                sources.insert(SourceName::new(name), source);
            }
        }
        sources
    }
}
