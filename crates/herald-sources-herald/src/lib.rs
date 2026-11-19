//! Another herald's feed as a source (ADR-0049): each page it lists is
//! read only once a key trusted for the peer signed the answer, and each
//! envelope on it only once its proof holds for the instance that made it.
//! An identity the source does not grant is taken under
//! `herald/<peer>/`, and its event's origin names the peer, never the
//! forge it claims.

mod options;
mod pull;
mod push;

pub use options::{HeraldFile, HeraldOptions, NAMESPACE, Verify};
pub use pull::{CLAIMED_FORGE, CLAIMED_HOST, CLAIMED_REPOSITORY};
pub use push::Unreceived;

use std::collections::BTreeMap;
use std::sync::Arc;

use herald_core::{Grant, Kind, SecretRef};
use herald_kinds::{
    Bound, Credential, Fault, Peering, Polling, Receive, Remembers, Source, SourceConfig,
    SourceKind,
};
use herald_peers::feed::{IDENTITY_MOST, PAGE_MOST, PROJECT_MOST};

use crate::pull::Pulled;
use crate::push::Inbox;

/// The kind of another herald's feed, as the config spells it.
pub const HERALD: &str = "herald";

/// The most bytes the name of an instance an identity is taken under
/// takes: a domain's.
const MAKER_MOST: usize = 253;

/// The most bytes one identity's proof takes in a cursor beside the
/// identity: when it was made, what it withdrew and the instance that made
/// it.
const PROOF_BYTES: usize = 512;

/// The most bytes one identity a cursor remembers takes beside its proof
/// and project: written once where it is seen, once with the digest of its
/// notes, and once with its project.
const SEEN_BYTES: usize = 128;

/// Another herald's feed, as a kind of source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeraldKind {}

impl SourceKind for HeraldKind {
    const NAME: &str = HERALD;
    const DESCRIPTION: &str =
        "Another herald's feed, each answer and envelope checked against the keys trusted for it.";

    type File = HeraldFile;
    type Options = HeraldOptions;

    fn checked(file: HeraldFile) -> Result<HeraldOptions, Fault> {
        HeraldOptions::of(file)
    }
}

impl SourceConfig for HeraldOptions {
    fn grants(&self) -> Option<&BTreeMap<String, Grant>> {
        Some(&self.grants)
    }

    fn kind(&self) -> &'static str {
        HERALD
    }

    fn token(&self) -> Option<&SecretRef> {
        None
    }

    fn kinds(&self) -> Vec<Kind> {
        Vec::new()
    }

    fn any_kind(&self) -> bool {
        true
    }

    fn backfill(&self) -> usize {
        self.backfill
    }

    fn origins(&self) -> bool {
        true
    }

    fn peering(&self) -> Option<Peering<'_>> {
        self.pulls.then(|| self.reads())
    }

    fn joined(&self, url: &str) -> Option<Source> {
        self.peer
            .is_some()
            .then(|| Source::new(self.served_at(url)))
    }

    fn share(&self) -> Option<usize> {
        Some(self.peer_share)
    }

    fn stale_after(&self) -> Option<jiff::SignedDuration> {
        self.fixed.then_some(self.stale_after)
    }

    fn receiver(&self) -> Option<Arc<dyn Receive>> {
        (!self.grants.is_empty()).then(|| Arc::new(Inbox(self.clone())) as Arc<dyn Receive>)
    }

    fn remembers(&self, peers: usize) -> Option<Remembers> {
        let (each, tombstone) = self.places();
        Some(Remembers {
            identities: self.pages.saturating_mul(PAGE_MOST),
            each,
            key: "pages",
            bound: self.bound,
            per: PAGE_MOST,
            beside: self
                .peer_share
                .saturating_mul(self.makers(peers))
                .saturating_add(self.tombstones)
                .saturating_mul(tombstone),
            besides: &["tombstones", "peer_share"],
        })
    }

    fn fitted(&self, most: usize, peers: usize) -> Option<Source> {
        let (each, tombstone) = self.places();
        let page = each.saturating_mul(PAGE_MOST);
        let room = most.saturating_sub(Remembers::REST).checked_sub(page)?;
        let makers = self.makers(peers);
        let wanted = self
            .peer_share
            .saturating_mul(makers)
            .saturating_add(self.tombstones);
        let kept = (room / tombstone).min(wanted);
        let tombstones = self.tombstones.saturating_mul(kept) / wanted;
        let peer_share = ((kept - tombstones) / makers).min(self.peer_share);
        let held = peer_share.saturating_mul(makers).saturating_add(tombstones);
        let pages = ((room - held * tombstone) / page + 1).min(self.pages);
        (tombstones > 0 && peer_share > 0 && self.backfill <= pages * PAGE_MOST).then(|| {
            Source::new(Self {
                pages,
                tombstones,
                peer_tombstones: self.peer_tombstones.min(tombstones),
                peer_share,
                bound: Bound::Fitted,
                ..self.clone()
            })
        })
    }

    fn poller(&self, _: Option<Credential>, _: bool) -> Polling {
        if self.pulls {
            Polling::Pull(Arc::new(Pulled::new(self)))
        } else {
            Polling::Unpolled
        }
    }
}

impl HeraldOptions {
    /// How many identities its cursor remembers that are no tombstones.
    #[must_use]
    pub fn remembered(&self) -> usize {
        self.pages.saturating_mul(PAGE_MOST)
    }

    /// How many instances its cursor holds tombstones of identities it took
    /// of, where `[peers]` names `peers` instances: the one it pulls, and
    /// with `verify = "hop"` each other peer too.
    #[must_use]
    pub fn makers(&self, peers: usize) -> usize {
        match (self.verify, &self.peer) {
            (Verify::Origin, _) => 1,
            (Verify::Hop, Some(_)) => peers.max(1),
            (Verify::Hop, None) => peers.saturating_add(1),
        }
    }

    /// The most bytes one identity its cursor remembers takes, and one
    /// tombstone it keeps.
    fn places(&self) -> (usize, usize) {
        let identity = NAMESPACE.len() + 1 + self.name.len().max(MAKER_MOST) + 1 + IDENTITY_MOST;
        (
            identity
                .saturating_mul(4)
                .saturating_add(PROJECT_MOST + SEEN_BYTES + PROOF_BYTES),
            identity.saturating_add(PROOF_BYTES),
        )
    }
}

#[cfg(test)]
mod tests;
