//! `[destinations]`: each destination read by its kind, and checked against
//! the store, the templates and `[http]`.

use std::collections::BTreeMap;
use std::ops::Range;

use herald_core::{DestinationName, RecordKind, TemplateName};
use herald_kinds::{Destination, Store, StoreKind};
use herald_store_git_branch::GitBranchKind;

use super::templates::Written;
use super::{Loader, Value, list};
use crate::kinds::At;

impl Loader<'_> {
    pub(super) fn destinations(
        &mut self,
        value: Option<Value<'_>>,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> BTreeMap<DestinationName, (Destination, Range<usize>)> {
        let mut destinations = BTreeMap::new();
        for (name, value) in self.named(value, "destinations") {
            let path = format!("destinations.{name}");
            let span = value.span();
            let kinds = self.kinds;
            let names = kinds.destination_kinds();
            let Some((kind, rest)) = self.kinded(value, &path, &names) else {
                continue;
            };
            let destination = kinds.destination_named(&kind).and_then(|kind| {
                kind.parse(
                    self,
                    rest,
                    At {
                        path: &path,
                        span: &span,
                    },
                    read,
                )
            });
            let Some(destination) = destination else {
                continue;
            };
            destinations.insert(DestinationName::new(name), (destination, span));
        }
        destinations
    }

    /// Whether every session a destination holds open waits less than
    /// `[http] total` allows any answer.
    pub(super) fn outlasted(
        &mut self,
        destinations: &BTreeMap<DestinationName, (Destination, Range<usize>)>,
        http: &herald_ports::Outbound,
    ) {
        let kinds = self.kinds;
        for (name, (destination, span)) in destinations {
            let Some(fault) = kinds
                .destination_named(destination.kind())
                .and_then(|kind| kind.outlasts(destination, http.total))
            else {
                continue;
            };
            let path = format!("destinations.{}", name.as_str());
            self.fault(At { path: &path, span }, &fault);
        }
    }

    /// Whether every wait a destination sets is long enough.
    pub(super) fn waited(
        &mut self,
        destinations: &BTreeMap<DestinationName, (Destination, Range<usize>)>,
    ) {
        for (name, (destination, span)) in destinations {
            if let Some(Err(error)) = destination
                .waits_written()
                .map(|set| herald_core::Waits::check(&set))
            {
                self.error(span, &format!("destinations.{}", name.as_str()), error);
            }
        }
    }

    /// Whether every destination that answers commands names a template
    /// there is, and has a store that keeps what its commands answer with.
    pub(super) fn answered(
        &mut self,
        store: Option<&Store>,
        destinations: &BTreeMap<DestinationName, (Destination, Range<usize>)>,
        templates: &BTreeMap<TemplateName, Written>,
    ) {
        for (name, (destination, span)) in destinations {
            let Some(answering) = destination.answering() else {
                continue;
            };
            let path = format!("destinations.{}.interactions", name.as_str());
            if let Some(template) = &answering.template
                && !templates.contains_key(template)
            {
                let names: Vec<&str> = templates.keys().map(TemplateName::as_str).collect();
                self.error(
                    span,
                    &format!("{path}.template"),
                    format!(
                        "no template is named \"{}\"; the templates are {}",
                        template.as_str(),
                        list(&names)
                    ),
                );
            }
            if let Some(store) = store
                && !store.keeps(RecordKind::Latest)
            {
                let hint = store.kept_by(RecordKind::Latest).unwrap_or_else(|| {
                    format!(
                        "give a store that keeps what each project last announced, such as \"{}\"",
                        GitBranchKind::NAME
                    )
                });
                self.error(
                    span,
                    &path,
                    format!(
                        "the {} store keeps nothing commands answer with; {hint}",
                        store.kind()
                    ),
                );
            }
        }
    }
}
