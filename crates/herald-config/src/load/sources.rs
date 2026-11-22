//! `[sources]`: each source read by its kind.

use std::collections::BTreeMap;
use std::ops::Range;

use herald_core::{DestinationName, Kind, SourceName};
use herald_kinds::{Destination, Source};

use super::route::Read;
use super::{Loader, Value, list};
use crate::config::{Doctor, Guard};
use crate::kinds::At;

impl Loader<'_> {
    pub(super) fn sources(
        &mut self,
        value: Option<Value<'_>>,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> (
        BTreeMap<SourceName, Source>,
        BTreeMap<SourceName, Range<usize>>,
    ) {
        let mut sources = BTreeMap::new();
        let mut spans = BTreeMap::new();
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
                spans.insert(SourceName::new(&name), span);
                sources.insert(SourceName::new(name), source);
            }
        }
        (sources, spans)
    }

    /// Every route over `sources` and `destinations` checked as `doctor`
    /// guards it: each source exposed, each kind unstated, each feed
    /// unaddressed, and each route a source names.
    pub(super) fn guarded(
        &mut self,
        (sources, spans): (
            &BTreeMap<SourceName, Source>,
            &BTreeMap<SourceName, Range<usize>>,
        ),
        destinations: &BTreeMap<DestinationName, (Destination, Range<usize>)>,
        routes: &[Read],
        doctor: Doctor,
    ) {
        self.exposed(sources, routes, doctor);
        self.unstated(routes, doctor);
        self.unaddressed(destinations, routes);
        self.routed((sources, spans), routes, doctor);
    }

    /// Each route a source names for some of its events, refused where no
    /// route has its id, or where it is not declared private, the source
    /// may take private or internal repositories, and `[doctor]
    /// private_to_public` refuses (ADR-0029).
    fn routed(
        &mut self,
        (sources, spans): (
            &BTreeMap<SourceName, Source>,
            &BTreeMap<SourceName, Range<usize>>,
        ),
        routes: &[Read],
        doctor: Doctor,
    ) {
        for (name, source) in sources {
            let span = spans.get(name).cloned().unwrap_or_default();
            for (key, id) in source.routed() {
                let path = format!("sources.{}.{key}", name.as_str());
                let Some(read) = routes
                    .iter()
                    .find(|read| read.config.route.id.as_str() == id)
                else {
                    self.error(
                        &span,
                        &path,
                        format!("there is no route with the id \"{id}\"; name a route's id"),
                    );
                    continue;
                };
                if doctor.private_to_public == Guard::Refuse
                    && !read.config.private
                    && source.unlisted()
                {
                    self.error(&span, &path, format!(
                        "the route {id} is not declared private, and this source may take private or internal repositories; set private = true on the route where its place is private, or set [doctor] private_to_public = \"warn\""
                    ));
                }
            }
        }
    }

    /// Each route that takes a private kind's events without saying whether
    /// its place is private, refused where `[doctor] private_kinds`
    /// refuses.
    fn unstated(&mut self, routes: &[Read], doctor: Doctor) {
        if doctor.private_kinds != Guard::Refuse {
            return;
        }
        for read in routes
            .iter()
            .filter(|read| !read.config.unstated.is_empty())
        {
            let kinds: Vec<&str> = read.config.unstated.iter().map(Kind::as_str).collect();
            self.error(
                &read.id_span,
                &read.path,
                format!(
                    "this route takes {} events, which belong in private places, and says nothing of whether its place is private; set private = true where it is, private = false to send them to a public place on purpose, or set [doctor] private_kinds = \"warn\"",
                    list(&kinds)
                ),
            );
        }
    }

    /// Each route that takes a source's private or internal repositories to
    /// a place not declared private, refused where `[doctor]
    /// private_to_public` refuses (ADR-0029).
    fn exposed(&mut self, sources: &BTreeMap<SourceName, Source>, routes: &[Read], doctor: Doctor) {
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
