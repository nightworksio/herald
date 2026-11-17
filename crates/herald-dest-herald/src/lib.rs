//! herald's own feed (ADR-0046): a destination herald serves rather than
//! posts to. Each event a route takes is published as an envelope proved by
//! the instance's newest key, kept newest first in the store, and listed in
//! pages at `/herald/v1/feeds/<destination>`.

mod options;
mod reach;

pub use options::{FeedOptions, FeedRouteOptions};
pub use reach::Feed;

use herald_kinds::{DestinationKind, Fault, Placing};

/// The kind of herald's own feed, as the config spells it.
pub const HERALD: &str = "herald";

/// herald's own feed, as a kind of destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeraldKind {}

impl DestinationKind for HeraldKind {
    const NAME: &str = HERALD;
    const DESCRIPTION: &str = "A feed of herald's own, served on the [federation] domain: each event as an envelope proved by the instance's newest key.";
    const LABEL: &str = "herald feed";
    const ROUTE: &str = "HeraldRoute";
    const ROUTE_DESCRIPTION: &str = "A route publishing its events to a feed of herald's own.";

    type Options = FeedOptions;
    type Route = FeedRouteOptions;

    fn checked(
        options: FeedOptions,
        _read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<(FeedOptions, Vec<Fault>), Fault> {
        options.check().map(|options| (options, Vec::new()))
    }

    fn placed(_: &FeedOptions, _: &FeedRouteOptions, _: Placing<'_>) -> Vec<Fault> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests;
