use herald_core::{Delivery, Profile, SecretRef, Symbols, WaitOptions};
use herald_kinds::{
    Checking, DestinationConfig, DestinationKind, Fault, Placed, Placement, Placing, Reached,
};
use herald_ports::Secret;

use crate::options::{DestinationOptions, RouteOptions};
use crate::teams::{self, TEAMS, Teams, UNCHECKED};

/// The kind of a Microsoft Teams destination, as the config spells it.
pub const KIND: &str = "teams";

impl DestinationConfig for DestinationOptions {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn symbols_written(&self) -> Option<Symbols> {
        self.symbols
    }

    fn waits_written(&self) -> Option<WaitOptions> {
        self.waits
    }

    fn credential(&self) -> Option<&SecretRef> {
        Some(&self.webhook)
    }

    fn secret_refs(&self) -> Vec<&SecretRef> {
        vec![&self.webhook]
    }

    fn profile(&self, placed: &Placed) -> Option<(Profile, Option<Profile>)> {
        placed.of::<RouteOptions>()?;
        Some((teams::profile(), None))
    }

    fn public(&self, _: &Placed, _: &Delivery) -> Option<&'static str> {
        None
    }

    fn checking(&self, _: &Placed) -> Checking {
        Checking::Unchecked(UNCHECKED)
    }

    fn recalled(&self, written: &str) -> Option<Placed> {
        serde_json::from_str::<RouteOptions>(written)
            .ok()
            .map(Placed::new)
    }

    fn reach(&self, placed: &Placed, _: &str, secret: Secret) -> Option<Reached> {
        placed.of::<RouteOptions>()?;
        Some(Reached::new(Teams::new(secret).waiting(self.waits())))
    }
}

impl Placement for RouteOptions {
    fn written(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }

    fn place(&self) -> Option<&str> {
        None
    }
}

/// Microsoft Teams, as a kind of destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamsKind {}

impl DestinationKind for TeamsKind {
    const NAME: &str = KIND;
    const DESCRIPTION: &str = "Microsoft Teams, by Workflows webhook.";
    const LABEL: &str = TEAMS;
    const ROUTE: &str = "TeamsRoute";
    const ROUTE_DESCRIPTION: &str = "A route to a Microsoft Teams destination.";

    type Options = DestinationOptions;
    type Route = RouteOptions;

    const FIRST_RUN: &[herald_kinds::Variant] = crate::first_run::VARIANTS;

    fn placed(_: &DestinationOptions, _: &RouteOptions, _: Placing<'_>) -> Vec<Fault> {
        Vec::new()
    }
}
