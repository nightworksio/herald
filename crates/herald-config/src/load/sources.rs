//! `[sources]`: each source read by its kind.

use std::collections::BTreeMap;

use herald_core::SourceName;
use herald_kinds::Source;

use super::route::Read;
use super::{Loader, Value};
use crate::config::{Doctor, Guard};
use crate::kinds::At;

impl Loader<'_> {
    pub(super) fn sources(
        &mut self,
        value: Option<Value<'_>>,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> BTreeMap<SourceName, Source> {
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
                .and_then(|kind| kind.parse(self, rest, at, read));
            if let Some(source) = source {
                sources.insert(SourceName::new(name), source);
            }
        }
        sources
    }

    /// Each route that takes a source's private or internal repositories to
    /// a place not declared private, refused where `[doctor]
    /// private_to_public` refuses (ADR-0029).
    pub(super) fn exposed(
        &mut self,
        sources: &BTreeMap<SourceName, Source>,
        routes: &[Read],
        doctor: Doctor,
    ) {
        if doctor.private_to_public != Guard::Refuse {
            return;
        }
        for read in routes.iter().filter(|read| !read.config.private) {
            for name in &read.config.route.filter.sources {
                if sources.get(name).is_some_and(|source| source.unlisted()) {
                    self.error(
                        &read.id_span,
                        &read.path,
                        format!(
                            "this route takes {}'s private or internal repositories to a place not declared private; set private = true where its place is private, leave them out with the source's visibility, or set [doctor] private_to_public = \"warn\"",
                            name.as_str()
                        ),
                    );
                }
            }
        }
    }
}
