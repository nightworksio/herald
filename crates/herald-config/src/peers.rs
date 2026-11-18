//! `[peers.*]`: the other heralds this one trusts, each by the keys it pins,
//! how it reaches them and what bounds what they send.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use herald_core::{Grant, Per, Rate, Size, SourceName};
use herald_peers::keys::{Key, WELL_KNOWN};
use schemars::JsonSchema;
use serde::Deserialize;

/// Whether a peer's own signed statement may replace the keys pinned for
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Rotation {
    /// Only the operator pins a new key, so a stolen key cannot replace
    /// itself; a new key set is reported, and `herald peer add --update`
    /// pins it.
    #[default]
    Manual,
    /// A new key set the peer signs with a pinned key replaces the pinned
    /// keys, and doctor names the change.
    Signed,
}

impl Rotation {
    /// Its name, as the config writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Signed => "signed",
        }
    }
}

/// Another herald this one trusts: where it is, the keys pinned for it, and
/// what bounds what it sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    /// Where it is served: `https://`, its domain and the path its
    /// endpoints are under, with no trailing `/`.
    pub url: String,
    /// Its domain, which its keys' ids name.
    pub domain: String,
    /// The Ed25519 keys pinned for it, as its discovery document lists
    /// them.
    pub keys: Vec<Key>,
    /// Whether its own signed statement may replace its pinned keys.
    pub rotation: Rotation,
    /// How many requests it may make of this herald.
    pub rate: Rate,
    /// The most bytes of a body it sends that are read.
    pub most: usize,
    /// The most instances an object it sends may have passed through.
    pub hops: u8,
    /// Whether private events may go to it.
    pub private: bool,
    /// What it may do with each resource that grants it anything, by the
    /// source it names.
    pub grants: BTreeMap<SourceName, Grant>,
}

impl Peer {
    /// How many requests a peer may make where the config says nothing.
    pub const RATE: Rate = Rate::new(NonZeroU32::new(120).unwrap(), Per::Minute);
    /// The most requests a config may let a peer make: four times the
    /// default.
    pub const RATE_ALLOWED: Rate = Rate::new(
        Self::RATE.count.saturating_mul(NonZeroU32::new(4).unwrap()),
        Self::RATE.per,
    );
    /// The most bytes of a peer's body read where the config says nothing.
    pub const MOST: usize = 2 * Size::MIB;
    /// The most bytes a config may let a peer's body hold: four times the
    /// default.
    pub const MOST_ALLOWED: usize = Self::MOST * 4;
    /// How many instances an object may have passed through where the
    /// config says nothing.
    pub const HOPS: u8 = 2;
    /// The most hops a config may allow: four times the default.
    pub const HOPS_ALLOWED: u8 = Self::HOPS * 4;

    /// The prefix of a resource a grant names that is a source.
    pub const SOURCES: &str = "sources.";

    /// The URL of its discovery document.
    #[must_use]
    pub fn discovery(&self) -> String {
        Self::discovery_at(&self.url)
    }

    /// The URL of the discovery document of a peer served at `url`.
    #[must_use]
    pub fn discovery_at(url: &str) -> String {
        format!("{url}{WELL_KNOWN}")
    }
}
