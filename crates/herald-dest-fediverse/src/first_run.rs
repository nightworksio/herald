//! How a destination of this kind is first set up (ADR-0091).

use herald_kinds::{Ask, Lands, Variant};

const TOKEN: Ask = Ask::secret(
    "token",
    "The secret that holds the account's access token",
    "HERALD_MASTODON_TOKEN",
);

/// A Mastodon destination's first way in: an account's access token on its
/// server.
pub(crate) const VARIANTS: &[Variant] = &[Variant {
    name: "token",
    described: "an account's access token, granted write:statuses and read:statuses",
    reads_back: true,
    asks: &[
        Ask::text(
            "server",
            "The server's base URL",
            "https://mastodon.social",
            Lands::Destination,
        ),
        TOKEN,
    ],
}];
