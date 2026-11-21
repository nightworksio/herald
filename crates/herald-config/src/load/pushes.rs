//! Each push to a peer: proved and signed by `[federation]`'s keys on its
//! domain, or presenting a CI run's ID token (ADR-0064), joined to a peer
//! of `[peers]`, recorded in a store that keeps
//! records, and reached by no route declared private unless the peer takes
//! private events (ADR-0054).

use std::collections::BTreeMap;
use std::ops::Range;

use herald_core::{Address, DestinationName};
use herald_kinds::{ConfiguredStore, Destination, Peered, Presented, StoreKind};
use herald_store_git_branch::GitBranchKind;

use super::Loader;
use crate::config::Federation;
use crate::peers::Peer;

impl Loader<'_> {
    /// Whether every push is proved and signed by `federation`, unless a
    /// CI run's ID token stands for it, and recorded by `store`, each
    /// joined to the peer of `peers` it names.
    pub(super) fn pushes(
        &mut self,
        (store, federation): (Option<&ConfiguredStore>, &Federation),
        peers: &BTreeMap<String, Peer>,
        destinations: &mut BTreeMap<DestinationName, (Destination, Range<usize>)>,
    ) {
        let presented = Presented::default();
        for (name, (destination, span)) in destinations.iter_mut() {
            let Some((peer, id_token, audience)) = destination.pushing().map(|pushing| {
                (
                    pushing.peer.to_owned(),
                    pushing.id_token,
                    pushing.audience.map(str::to_owned),
                )
            }) else {
                continue;
            };
            let path = format!("destinations.{}", name.as_str());
            if !id_token && (federation.domain.is_none() || federation.keys.is_empty()) {
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
            if let Some(audience) = audience.filter(|audience| !aimed(audience, found)) {
                self.error(
                    span,
                    &format!("{path}.audience"),
                    format!(
                        "\"{audience}\" is not on the origin of peers.{peer}, {}, so a token meant for another herald would be handed to this one, which could present it there; give an audience on the peer's own origin, or leave audience out",
                        found.url
                    ),
                );
            }
            if let Some(peered) = destination.peered(Peered {
                url: found.url.clone(),
                private: found.private,
                presented: presented.clone(),
            }) {
                *destination = peered;
            }
        }
    }
}

/// Whether `audience` is on the origin of `peer`'s URL, which its domain
/// is the host of, and names no user.
fn aimed(audience: &str, peer: &Peer) -> bool {
    let origin = |url: &str| {
        Address::parse(url)
            .filter(|parsed| !parsed.names_a_user())
            .map(|parsed| parsed.origin())
    };
    origin(audience).is_some_and(|aimed| origin(&peer.url) == Some(aimed))
}
