//! Each push to a peer: proved and signed by `[federation]`'s keys on its
//! domain, joined to a peer of `[peers]`, recorded in a store that keeps
//! records, and reached by no route declared private unless the peer takes
//! private events (ADR-0054).

use std::collections::BTreeMap;
use std::ops::Range;

use herald_core::DestinationName;
use herald_kinds::{ConfiguredStore, Destination, Peered, StoreKind};
use herald_store_git_branch::GitBranchKind;

use super::Loader;
use super::route::Read;
use crate::config::{Doctor, Federation, Guard};
use crate::peers::Peer;

impl Loader<'_> {
    /// Whether every push is proved and signed by `federation` and recorded
    /// by `store`, each joined to the peer of `peers` it names.
    pub(super) fn pushes(
        &mut self,
        (store, federation): (Option<&ConfiguredStore>, &Federation),
        peers: &BTreeMap<String, Peer>,
        destinations: &mut BTreeMap<DestinationName, (Destination, Range<usize>)>,
    ) {
        for (name, (destination, span)) in destinations.iter_mut() {
            let Some(peer) = destination.pushing().map(|pushing| pushing.peer.to_owned()) else {
                continue;
            };
            let path = format!("destinations.{}", name.as_str());
            if federation.domain.is_none() || federation.keys.is_empty() {
                self.error(
                    span,
                    &path,
                    "a push to a peer is proved and signed by [federation]'s keys on its domain; give domain and keys in [federation]",
                );
            }
            if let Some(store) = store.filter(|store| store.reads_posts()) {
                self.error(
                    span,
                    &path,
                    format!(
                        "the {} store keeps what was announced in the posts it reads back, and a push reads none; give a store that keeps records, such as \"{}\"",
                        store.kind(),
                        GitBranchKind::NAME
                    ),
                );
            }
            let Some(found) = peers.get(&peer) else {
                self.error(
                    span,
                    &format!("{path}.peer"),
                    format!(
                        "there is no peer \"{peer}\" in [peers]; `herald peer add <url>` pins one"
                    ),
                );
                continue;
            };
            if let Some(peered) = destination.peered(Peered {
                url: found.url.clone(),
                private: found.private,
            }) {
                *destination = peered;
            }
        }
    }

    /// Whether no route declared private reaches a push to a peer that
    /// takes no private events, where `[doctor] private_to_public` refuses
    /// it.
    pub(super) fn unpushed(
        &mut self,
        destinations: &BTreeMap<DestinationName, (Destination, Range<usize>)>,
        peers: &BTreeMap<String, Peer>,
        routes: &[Read],
        doctor: Doctor,
    ) {
        if doctor.private_to_public != Guard::Refuse {
            return;
        }
        for read in routes.iter().filter(|read| read.config.private) {
            let refused = destinations
                .get(&read.config.route.destination)
                .and_then(|(destination, _)| destination.pushing())
                .and_then(|pushing| peers.get(pushing.peer))
                .is_some_and(|peer| !peer.private);
            if refused {
                self.error(
                    &read.id_span,
                    &format!("{}.private", read.path),
                    "this route is declared private, and its push goes to a peer that takes no private events; set private = true on the peer in [peers] if it may take them, route private events elsewhere, or set [doctor] private_to_public = \"warn\"",
                );
            }
        }
    }
}
