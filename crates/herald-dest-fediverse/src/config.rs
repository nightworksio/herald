use std::any::Any;

use herald_core::{Profile, SecretRef, Symbols, WaitOptions, Waits};
use herald_kinds::{DestinationConfig, Placed, Placement, Reached, same};
use herald_ports::Secret;

use crate::activitypub::{ActivityPub, ActorOptions, ActorRouteOptions, actor_profile};
use crate::mastodon::{self, Mastodon};
use crate::options::{DestinationOptions, RouteOptions};

/// The kind of a Mastodon destination, as the config spells it.
pub const MASTODON: &str = "mastodon";
/// The kind of herald's own ActivityPub actors, as the config spells it.
pub const ACTIVITYPUB: &str = "activitypub";

impl DestinationConfig for DestinationOptions {
    fn kind(&self) -> &'static str {
        MASTODON
    }

    fn symbols_written(&self) -> Option<Symbols> {
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

    fn profile(&self, placed: &Placed) -> Option<(Profile, Option<Profile>)> {
        Some((mastodon::profile(placed.of::<RouteOptions>()?), None))
    }

    fn reach(&self, placed: &Placed, _: &str, secret: Secret) -> Option<Reached> {
        let placed = placed.of::<RouteOptions>()?;
        Some(Reached::new(
            Mastodon::new(&self.server, secret, placed.clone())
                .reading_back(self.read_back.unwrap_or_default())
                .waiting(self.waits()),
        ))
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

    fn symbols_written(&self) -> Option<Symbols> {
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

    fn profile(&self, placed: &Placed) -> Option<(Profile, Option<Profile>)> {
        placed.of::<ActorRouteOptions>()?;
        Some((actor_profile(), None))
    }

    fn reach(&self, placed: &Placed, base: &str, secret: Secret) -> Option<Reached> {
        let placed = placed.of::<ActorRouteOptions>()?;
        Some(Reached::new(
            ActivityPub::new(&self.domain, base, secret, placed.clone())
                .waiting(self.waits())
                .paging(self.outbox_page())
                .governed(self.policy()),
        ))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}

impl Placement for RouteOptions {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}

impl Placement for ActorRouteOptions {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        same(self, other)
    }
}
