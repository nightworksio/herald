//! How a destination of this kind is first set up (ADR-0091).

use herald_kinds::{Ask, Lands, Variant};

const HOMESERVER: Ask = Ask::text(
    "homeserver",
    "The homeserver's base URL",
    "https://matrix.org",
    Lands::Destination,
);

const TOKEN: Ask = Ask::secret(
    "token",
    "The secret that holds the bot's access token",
    "HERALD_MATRIX_TOKEN",
);

const ROOM: Ask = Ask::text(
    "room",
    "The id of the room the bot has joined",
    "!abcdefg:matrix.org",
    Lands::Route,
);

/// A Matrix destination's first way in: a bot account's access token on its
/// homeserver, posting to a room it has joined.
pub(crate) const VARIANTS: &[Variant] = &[Variant {
    name: "bot",
    described: "a bot account's access token: it threads, edits and reads its posts back",
    reads_back: true,
    asks: &[HOMESERVER, TOKEN, ROOM],
}];
