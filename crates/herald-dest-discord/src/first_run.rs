//! How a destination of this kind is first set up (ADR-0091).

use herald_kinds::{Answer, Ask, Lands, Variant};

/// A Discord destination's first ways in: a channel webhook, which needs
/// nothing but its URL, then a bot.
pub(crate) const VARIANTS: &[Variant] = &[
    Variant {
        name: "webhook",
        described: "a channel webhook's URL: it posts only, where the webhook was made",
        reads_back: false,
        asks: &[Ask {
            key: "webhook",
            asked: "The secret that holds the webhook's URL",
            example: "HERALD_DISCORD_WEBHOOK",
            answer: Answer::Secret {
                default: "HERALD_DISCORD_WEBHOOK",
            },
            lands: Lands::Destination,
        }],
    },
    Variant {
        name: "bot",
        described: "a bot token: it threads, crossposts, pins and reads its posts back",
        reads_back: true,
        asks: &[
            Ask {
                key: "token",
                asked: "The secret that holds the bot token",
                example: "HERALD_DISCORD_TOKEN",
                answer: Answer::Secret {
                    default: "HERALD_DISCORD_TOKEN",
                },
                lands: Lands::Destination,
            },
            Ask {
                key: "channel",
                asked: "The id of the channel the bot posts to",
                example: "1566184281406177531",
                answer: Answer::Text,
                lands: Lands::Route,
            },
        ],
    },
];
