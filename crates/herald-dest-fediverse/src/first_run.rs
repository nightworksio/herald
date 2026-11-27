//! How a destination of this kind is first set up (ADR-0091).

use herald_kinds::{Answer, Ask, Lands, Variant};

/// A Mastodon destination's first way in: an account's access token on its
/// server.
pub(crate) const VARIANTS: &[Variant] = &[Variant {
    name: "token",
    described: "an account's access token, granted write:statuses and read:statuses",
    reads_back: true,
    asks: &[
        Ask {
            key: "server",
            asked: "The server's base URL",
            example: "https://mastodon.social",
            answer: Answer::Text,
            lands: Lands::Destination,
        },
        Ask {
            key: "token",
            asked: "The secret that holds the account's access token",
            example: "HERALD_MASTODON_TOKEN",
            answer: Answer::Secret {
                default: "HERALD_MASTODON_TOKEN",
            },
            lands: Lands::Destination,
        },
    ],
}];
