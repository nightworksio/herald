//! Another herald's feed as a source (ADR-0049): each page it lists is
//! read only once a key trusted for the peer signed the answer, and each
//! envelope on it only once its proof holds for the instance that made it.
//! An identity the source does not grant is taken under
//! `herald/<peer>/`, and its event's origin names the peer, never the
//! forge it claims.

mod options;
mod pull;

pub use options::{HeraldFile, HeraldOptions, NAMESPACE, Verify};
pub use pull::{CLAIMED_FORGE, CLAIMED_HOST, CLAIMED_REPOSITORY};

use std::sync::Arc;

use herald_core::{Kind, SecretRef};
use herald_kinds::{Credential, Fault, Peering, Polling, Source, SourceConfig, SourceKind};

use crate::pull::Pulled;

/// The kind of another herald's feed, as the config spells it.
pub const HERALD: &str = "herald";

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
        Some(self.reads())
    }

    fn joined(&self, url: &str) -> Option<Source> {
        self.peer
            .is_some()
            .then(|| Source::new(self.served_at(url)))
    }

    fn poller(&self, _: Option<Credential>, _: bool) -> Polling {
        Polling::Pull(Arc::new(Pulled::new(self)))
    }
}

#[cfg(test)]
mod tests;
