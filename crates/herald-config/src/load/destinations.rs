//! `[destinations]`: each destination read by its kind, and checked against
//! the store, the templates and `[http]`.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ops::Range;

use herald_core::{DestinationName, RecordKind, TemplateName};
use herald_kinds::{ConfiguredStore, Destination, Inherited, StoreKind};
use herald_store_git_branch::GitBranchKind;
use toml::Spanned;
use toml::de::{DeTable, DeValue};

use super::templates::Written;
use super::{Loader, Value, list};
use crate::kinds::At;

/// `destinations` without where each is written.
pub(super) fn unspanned(
    destinations: BTreeMap<DestinationName, (Destination, Range<usize>)>,
) -> BTreeMap<DestinationName, Destination> {
    destinations
        .into_iter()
        .map(|(name, (destination, _))| (name, destination))
        .collect()
}

impl Loader<'_> {
    pub(super) fn destinations<'i>(
        &mut self,
        value: Option<Value<'i>>,
        inherited: &DeTable<'i>,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> BTreeMap<DestinationName, (Destination, Range<usize>)> {
        let mut destinations = BTreeMap::new();
        for (name, value) in self.named(value, "destinations") {
            let path = format!("destinations.{name}");
            let kinds = self.kinds;
            let names = kinds.destination_kinds();
            let Some((kind, span, mut table)) =
                self.kinded_table(value, &path, &names)
                    .and_then(|(kind, span, table)| {
                        Some((kinds.destination_named(&kind)?, span, table))
                    })
            else {
                continue;
            };
            if !self.inherit(
                &mut table,
                kind.inherits(),
                inherited,
                At {
                    path: &path,
                    span: &span,
                },
            ) {
                continue;
            }
            let rest = Spanned::new(span.clone(), DeValue::Table(table));
            let at = At {
                path: &path,
                span: &span,
            };
            let Some(destination) = kind.parse(self, rest, at, read) else {
                continue;
            };
            destinations.insert(DestinationName::new(name), (destination, span));
        }
        destinations
    }

    /// Whether `table` holds every key its kind needs once each key of
    /// `inherits` it leaves out is taken from `[federation]`, `inherited`.
    fn inherit<'i>(
        &mut self,
        table: &mut DeTable<'i>,
        inherits: &[Inherited],
        inherited: &DeTable<'i>,
        at: At<'_>,
    ) -> bool {
        let mut whole = true;
        for inheriting in inherits {
            if table.contains_key(inheriting.key) {
                continue;
            }
            if let Some((_, value)) = inherited.get_key_value(inheriting.from) {
                table.insert(
                    Spanned::new(value.span(), Cow::Borrowed(inheriting.key)),
                    value.clone(),
                );
            } else if inheriting.required {
                self.error(
                    at.span,
                    at.path,
                    format!(
                        "give its {}, or {} in [federation]",
                        inheriting.key, inheriting.from
                    ),
                );
                whole = false;
            }
        }
        whole
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
        store: Option<&ConfiguredStore>,
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
