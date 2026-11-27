//! How a destination of this kind is first set up (ADR-0091).

use herald_kinds::{Answer, Ask, Lands, Variant};

const DOMAIN: Ask = Ask {
    key: "domain",
    asked: "Which Slack does the workspace live on",
    example: "slack.com, or slack-gov.com for Slack's government cloud",
    answer: Answer::Choice {
        choices: &["slack.com", "slack-gov.com"],
    },
    lands: Lands::Destination,
};

/// A Slack destination's first ways in: an incoming webhook, which needs
/// nothing but its URL, then a bot.
pub(crate) const VARIANTS: &[Variant] = &[
    Variant {
        name: "webhook",
        described: "an incoming webhook's URL: it posts only, where the webhook was made",
        reads_back: false,
        asks: &[
            Ask {
                key: "webhook",
                asked: "The secret that holds the webhook's URL",
                example: "HERALD_SLACK_WEBHOOK",
                answer: Answer::Secret {
                    default: "HERALD_SLACK_WEBHOOK",
                },
                lands: Lands::Destination,
            },
            DOMAIN,
        ],
    },
    Variant {
        name: "bot",
        described: "a bot token (xoxb-…): it threads, pins and reads its posts back",
        reads_back: true,
        asks: &[
            Ask {
                key: "token",
                asked: "The secret that holds the bot token",
                example: "HERALD_SLACK_TOKEN",
                answer: Answer::Secret {
                    default: "HERALD_SLACK_TOKEN",
                },
                lands: Lands::Destination,
            },
            DOMAIN,
            Ask {
                key: "channel",
                asked: "The id of the channel the bot has joined",
                example: "C0123456789",
                answer: Answer::Text,
                lands: Lands::Route,
            },
        ],
    },
];
