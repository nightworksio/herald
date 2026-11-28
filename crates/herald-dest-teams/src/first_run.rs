//! How a destination of this kind is first set up (ADR-0091).

use herald_kinds::{Answer, Ask, Lands, Variant};

/// A Teams destination's first ways in: a Workflows webhook, which needs
/// nothing but its URL, then a bot.
pub(crate) const VARIANTS: &[Variant] = &[
    Variant {
        name: "webhook",
        described: "a Workflows webhook's URL: it posts only, to the channel or chat its workflow names",
        reads_back: false,
        asks: &[Ask::secret(
            "webhook",
            "The secret that holds the workflow's webhook URL",
            "HERALD_TEAMS_WEBHOOK",
        )],
    },
    Variant {
        name: "bot",
        described: "a bot's app registration: it edits its posts and can reply in their threads",
        reads_back: false,
        asks: &[
            Ask::text(
                "app_id",
                "The application (client) id of the bot's app registration",
                "00000000-0000-0000-0000-000000000000",
                Lands::Destination,
            ),
            Ask::secret(
                "app_password",
                "The secret that holds the bot's client secret",
                "HERALD_TEAMS_APP_PASSWORD",
            ),
            Ask::text(
                "tenant",
                "The id of the Microsoft Entra tenant the app is registered in",
                "contoso.onmicrosoft.com",
                Lands::Destination,
            ),
            Ask {
                key: "cloud",
                asked: "Which Microsoft cloud is the bot registered in",
                example: "public, or gcc-high or dod for Azure Government",
                answer: Answer::Choice {
                    choices: &["public", "gcc-high", "dod"],
                },
                lands: Lands::Destination,
            },
            Ask::text(
                "conversation",
                "The id of the channel or chat the bot's app is installed in",
                "19:…@thread.tacv2",
                Lands::Route,
            ),
        ],
    },
];
