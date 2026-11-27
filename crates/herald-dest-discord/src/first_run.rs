//! How a destination of this kind is first set up (ADR-0091).

use herald_kinds::{Ask, Lands, Variant};

/// A Discord destination's first ways in: a channel webhook, which needs
/// nothing but its URL, then a bot.
pub(crate) const VARIANTS: &[Variant] = &[
    Variant {
        name: "webhook",
        described: "a channel webhook's URL: it posts only, where the webhook was made",
        reads_back: false,
        asks: &[Ask::secret(
            "webhook",
            "The secret that holds the webhook's URL",
            "HERALD_DISCORD_WEBHOOK",
        )],
    },
    Variant {
        name: "bot",
        described: "a bot token: it threads, crossposts, pins and reads its posts back",
        reads_back: true,
        asks: &[
            Ask::secret(
                "token",
                "The secret that holds the bot token",
                "HERALD_DISCORD_TOKEN",
            ),
            Ask::text(
                "channel",
                "The id of the channel the bot posts to",
                "1566184281406177531",
                Lands::Route,
            ),
        ],
    },
];
