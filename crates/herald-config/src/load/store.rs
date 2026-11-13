//! `[store]`: where herald keeps its records and for how long, and whether
//! it keeps what each route needs.

use std::collections::BTreeMap;
use std::ops::Range;

use herald_core::{DestinationName, RecordKind, Retention};
use herald_kinds::{ConfiguredStore, Destination, StoreKind};
use herald_store_git_branch::GitBranchKind;
use toml::Spanned;
use toml::de::{DeTable, DeValue};

use super::route::Read;
use super::{Loader, Value, list};
use crate::file::{Keep, KeepFile};

impl Loader<'_> {
    pub(super) fn store(
        &mut self,
        value: Option<Value<'_>>,
    ) -> Option<(ConfiguredStore, Retention)> {
        let kinds = self.kinds;
        let Some(value) = value else {
            self.error(
                &(0..0),
                "store",
                format!(
                    "choose a store: [store] with kind = one of {}, such as \"{}\" on GitHub Actions",
                    list(&kinds.store_kinds()),
                    GitBranchKind::NAME
                ),
            );
            return None;
        };
        let (kind, span, table) = self.kinded_table(value, "store", &kinds.store_kinds())?;
        let kind = kinds.store_named(&kind)?;
        if !kind.retained() {
            let store = kind.parse(self, Spanned::new(span, DeValue::Table(table)))?;
            return Some((store, Retention::default()));
        }
        let (rest, keep) = kept(span, table);
        let retention = self.retention(keep);
        let store = kind.parse(self, rest)?;
        Some((store, retention?))
    }

    fn retention(&mut self, keep: Value<'_>) -> Option<Retention> {
        let file = self.typed::<KeepFile>(keep, "store")?;
        let defaults = Retention::default();
        let deliveries = file.deliveries.map_or(defaults.deliveries, |kept| {
            if kept.get_ref().0 < Retention::LONGEST_RETRY {
                self.error(
                    &kept.span(),
                    "store.keep_deliveries",
                    format!(
                        "keep a delivery's id at least \"{}d\", longer than any sender goes on retrying it",
                        Retention::LONGEST_RETRY.as_hours() / 24
                    ),
                );
            }
            kept.into_inner().0
        });
        Some(Retention {
            deliveries,
            ledger: file.ledger.map_or(defaults.ledger, Keep::kept),
            notes: file.notes.map_or(defaults.notes, Keep::kept),
        })
    }

    /// Whether every route that reaches followers or subscribers has a store
    /// that keeps them.
    pub(super) fn followed(
        &mut self,
        store: Option<&ConfiguredStore>,
        destinations: &BTreeMap<DestinationName, (Destination, Range<usize>)>,
        routes: &[Read],
    ) {
        let Some(store) = store else {
            return;
        };
        let elsewhere = |kind| {
            store.kept_by(kind).unwrap_or_else(|| {
                format!(
                    "give a store that keeps them, such as \"{}\"",
                    GitBranchKind::NAME
                )
            })
        };
        for read in routes {
            let route = &read.config.route;
            let follows = destinations
                .get(&route.destination)
                .is_some_and(|(destination, _)| destination.acts());
            if (follows || route.delivery.mentions.subscribers)
                && !store.keeps(RecordKind::Subscriptions)
            {
                self.error(
                    &read.id_span,
                    &read.path,
                    format!(
                        "the {} store keeps no subscriptions, so this route has no followers or subscribers to reach; {}",
                        store.kind(),
                        elsewhere(RecordKind::Subscriptions)
                    ),
                );
            } else if follows && !store.keeps(RecordKind::Notes) {
                self.error(
                    &read.id_span,
                    &read.path,
                    format!(
                        "the {} store keeps no notes, which this route's actor serves its announcements as; {}",
                        store.kind(),
                        elsewhere(RecordKind::Notes)
                    ),
                );
            }
        }
    }
}

/// `options`, a store's table written at `span`, apart from how long it
/// keeps what herald records.
fn kept(span: Range<usize>, mut options: DeTable<'_>) -> (Value<'_>, Value<'_>) {
    let mut keep = DeTable::new();
    for key in KeepFile::KEYS {
        if let Some((name, value)) = options.remove_entry(key) {
            keep.insert(name, value);
        }
    }
    (
        Spanned::new(span.clone(), DeValue::Table(options)),
        Spanned::new(span, DeValue::Table(keep)),
    )
}
