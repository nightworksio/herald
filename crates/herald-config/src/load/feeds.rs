//! Each feed of herald's own: served on `[federation]`'s domain and proved
//! by its keys, kept in a store that keeps records and fitted to the most
//! its records hold, and reached by no route declared private (ADR-0046).

use std::collections::BTreeMap;
use std::ops::Range;

use herald_core::DestinationName;
use herald_kinds::{Bound, Checking, ConfiguredStore, Destination, StoreKind};
use herald_store_git_branch::GitBranchKind;

use super::Loader;
use super::route::Read;
use crate::config::Federation;

/// Why a route vouches for nothing where it does not say it is private.
const UNPRIVATELY_VOUCHED: &str = "vouch_unchecked vouches for a route that says private = true; add private = true, or remove vouch_unchecked";

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

    /// Whether no route declared private posts where anyone may read it,
    /// as its destination decides for each kind: a feed, a push to a peer
    /// that takes no private events, herald's own ActivityPub actor, a
    /// public Mastodon status or a crosspost. No setting widens it; and
    /// whether each route vouching for its place says `private = true` and
    /// posts where herald cannot check who may read (ADR-0067).
    pub(super) fn unaddressed(
        &mut self,
        destinations: &BTreeMap<DestinationName, (Destination, Range<usize>)>,
        routes: &[Read],
    ) {
        for read in routes {
            let Some((destination, _)) = destinations.get(&read.config.route.destination) else {
                continue;
            };
            let public = destination.public(&read.config.options, &read.config.route.delivery);
            if let (true, Some(reason)) = (read.config.private, public) {
                self.error(
                    &read.id_span,
                    &format!("{}.private", read.path),
                    format!("this route is declared private, but {reason}; remove private = true, and route private events elsewhere"),
                );
            }
            if !read.config.vouched {
                continue;
            }
            let refused = match destination.checking(&read.config.options) {
                _ if !read.config.private => Some(UNPRIVATELY_VOUCHED.to_owned()),
                Checking::Unchecked(_) => None,
                Checking::Asked { asks, .. } => Some(format!(
                    "herald asks {asks} before each run's first advisory send, so there is nothing to vouch for; remove vouch_unchecked"
                )),
                Checking::Unadvised => Some(
                    "its kind of destination is never sent an advisories section, so there is nothing to vouch for; remove vouch_unchecked".to_owned(),
                ),
            };
            if let Some(reason) = refused {
                self.error(
                    &read.id_span,
                    &format!("{}.vouch_unchecked", read.path),
                    reason,
                );
            }
        }
    }
}
