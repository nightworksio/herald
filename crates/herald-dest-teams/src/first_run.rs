//! How a destination of this kind is first set up (ADR-0091).

use herald_kinds::{Ask, Variant};

/// A Teams destination's first way in: a Workflows webhook, which needs
/// nothing but its URL.
pub(crate) const VARIANTS: &[Variant] = &[Variant {
    name: "webhook",
    described: "a Workflows webhook's URL: it posts only, to the channel or chat its workflow names",
    reads_back: false,
    asks: &[Ask::secret(
        "webhook",
        "The secret that holds the workflow's webhook URL",
        "HERALD_TEAMS_WEBHOOK",
    )],
}];
