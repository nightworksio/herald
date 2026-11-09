use std::any::Any;

use herald_core::{SecretRef, Symbols, WaitOptions, Waits};
use herald_kinds::{DestinationConfig, same};

use crate::activitypub::ActorOptions;
use crate::mastodon::Mastodon;
use crate::options::DestinationOptions;

/// The kind of a Mastodon destination, as the config spells it.
pub const MASTODON: &str = "mastodon";
/// The kind of herald's own ActivityPub actors, as the config spells it.
pub const ACTIVITYPUB: &str = "activitypub";

impl DestinationConfig for DestinationOptions {
    fn kind(&self) -> &'static str {
        MASTODON
    }

    fn symbols(&self) -> Option<Symbols> {
        self.symbols
    }

    fn waits_written(&self) -> Option<WaitOptions> {
        self.waits
    }

    fn platform_waits(&self) -> Waits {
        Mastodon::WAITS
    }

    fn credential(&self) -> Option<&SecretRef> {
        Some(&self.token)
    }

    fn secret_refs(&self) -> Vec<&SecretRef> {
        vec![&self.token]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}

impl DestinationConfig for ActorOptions {
    fn kind(&self) -> &'static str {
        ACTIVITYPUB
    }

    fn symbols(&self) -> Option<Symbols> {
        self.symbols
    }

    fn waits_written(&self) -> Option<WaitOptions> {
        self.waits
    }

    fn credential(&self) -> Option<&SecretRef> {
        Some(&self.key)
    }

    fn secret_refs(&self) -> Vec<&SecretRef> {
        vec![&self.key]
    }

    fn acts(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}
