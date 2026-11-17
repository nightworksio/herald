//! Each feed of herald's own: served on `[federation]`'s domain and proved
//! by its keys, kept in a store that keeps records and fitted to the most
//! its records hold, and reached by no route declared private (ADR-0046).

use std::collections::BTreeMap;
use std::ops::Range;

use herald_core::DestinationName;
use herald_kinds::{Bound, ConfiguredStore, Destination, StoreKind};
use herald_store_git_branch::GitBranchKind;

use super::Loader;
use super::route::Read;
use crate::config::{Doctor, Federation, Guard};

impl Loader<'_> {
    /// Whether every feed is served and proved by `federation` and kept by
    /// `store`, each whose options name no bound fitted to the most the
    /// store's records hold.
    pub(super) fn feeds(
        &mut self,
        store: Option<&ConfiguredStore>,
        federation: &Federation,
        destinations: &mut BTreeMap<DestinationName, (Destination, Range<usize>)>,
    ) {
        for (name, (destination, span)) in destinations.iter_mut() {
            let Some(feeding) = destination.feeding() else {
                continue;
            };
            let path = format!("destinations.{}", name.as_str());
            if federation.domain.is_none() || federation.keys.is_empty() {
                self.error(
                    span,
                    &path,
                    "a herald feed is served on [federation]'s domain and its envelopes proved by its keys; give domain and keys in [federation]",
                );
            }
            let Some(store) = store else {
                continue;
            };
            if store.reads_posts() {
                self.error(
                    span,
                    &path,
                    format!(
                        "the {} store keeps no feed, and a herald feed is kept as a record; give a store that keeps records, such as \"{}\"",
                        store.kind(),
                        GitBranchKind::NAME
                    ),
                );
                continue;
            }
            let Some(most) = store.most_record().filter(|most| !feeding.fits(*most)) else {
                continue;
            };
            let fitting = feeding.fitting(most);
            match (feeding.bound == Bound::Default && fitting > 0)
                .then(|| destination.fitted(fitting))
                .flatten()
            {
                Some(fitted) => *destination = fitted,
                None => self.error(
                    span,
                    &format!("{path}.keep"),
                    format!(
                        "the {} store keeps at most {most} bytes in a record, and this feed may take {} keeping {} envelopes; make keep at most {fitting} or envelope smaller, or give a store that keeps more",
                        store.kind(),
                        feeding.bytes(),
                        feeding.keep,
                    ),
                ),
            }
        }
    }

    /// Whether no route declared private reaches a feed, which anyone may
    /// read, where `[doctor] private_to_public` refuses it.
    pub(super) fn unfed(
        &mut self,
        destinations: &BTreeMap<DestinationName, (Destination, Range<usize>)>,
        routes: &[Read],
        doctor: Doctor,
    ) {
        if doctor.private_to_public != Guard::Refuse {
            return;
        }
        for read in routes.iter().filter(|read| read.config.private) {
            let feeds = destinations
                .get(&read.config.route.destination)
                .is_some_and(|(destination, _)| destination.feeding().is_some());
            if feeds {
                self.error(
                    &read.id_span,
                    &format!("{}.private", read.path),
                    "this route is declared private, and a herald feed is served to anyone and takes only public events; route private events elsewhere, or set [doctor] private_to_public = \"warn\"",
                );
            }
        }
    }
}
