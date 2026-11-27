//! How a destination of this kind is first set up (ADR-0091).

use herald_kinds::{Answer, Ask, Lands, Variant};

/// A Matrix destination's first way in: a bot account's access token on its
/// homeserver.
pub(crate) const VARIANTS: &[Variant] = &[Variant {
    name: "bot",
    described: "a bot account's access token: it threads, edits and reads its posts back",
    reads_back: true,
    asks: &[
        Ask {
            key: "homeserver",
            asked: "The homeserver's base URL",
            example: "https://matrix.org",
            answer: Answer::Text,
            lands: Lands::Destination,
        },
        Ask {
            key: "token",
            asked: "The secret that holds the bot's access token",
            example: "HERALD_MATRIX_TOKEN",
            answer: Answer::Secret {
                default: "HERALD_MATRIX_TOKEN",
            },
            lands: Lands::Destination,
        },
        Ask {
            key: "room",
            asked: "The id of the room the bot has joined",
            example: "!abcdefg:matrix.org",
            answer: Answer::Text,
            lands: Lands::Route,
        },
    ],
}];
