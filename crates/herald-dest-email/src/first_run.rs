//! How a destination of this kind is first set up (ADR-0091).

use herald_kinds::{Answer, Ask, Lands, Variant};

/// An email destination's first ways in: Amazon SES and Mailgun.
pub(crate) const VARIANTS: &[Variant] = &[
    Variant {
        name: "ses",
        described: "Amazon SES in one region, with an access key that may send from a verified address",
        reads_back: false,
        asks: &[
            Ask {
                key: "transport",
                asked: "How each message leaves",
                example: "ses",
                answer: Answer::Fixed { value: "ses" },
                lands: Lands::Destination,
            },
            Ask::text(
                "from",
                "The address each message is from, verified in Amazon SES",
                "herald <releases@example.org>",
                Lands::Destination,
            ),
            Ask::text(
                "region",
                "The AWS region Amazon SES sends from",
                "eu-west-1",
                Lands::Destination,
            ),
            Ask::secret(
                "access_key_id",
                "The secret that holds the access key's id",
                "AWS_ACCESS_KEY_ID",
            ),
            Ask::secret(
                "secret_access_key",
                "The secret that holds the secret access key",
                "AWS_SECRET_ACCESS_KEY",
            ),
            Ask::text(
                "to",
                "The address each message goes to",
                "team@example.org",
                Lands::Route,
            ),
        ],
    },
    Variant {
        name: "mailgun",
        described: "Mailgun, with a domain sending key of a domain it verified",
        reads_back: false,
        asks: &[
            Ask {
                key: "transport",
                asked: "How each message leaves",
                example: "mailgun",
                answer: Answer::Fixed { value: "mailgun" },
                lands: Lands::Destination,
            },
            Ask::text(
                "from",
                "The address each message is from, of the domain Mailgun sends as",
                "herald <releases@mg.example.org>",
                Lands::Destination,
            ),
            Ask::text(
                "region",
                "The Mailgun region the domain was made in, us or eu",
                "eu",
                Lands::Destination,
            ),
            Ask::text(
                "domain",
                "The Mailgun domain each message is sent as",
                "mg.example.org",
                Lands::Destination,
            ),
            Ask::secret(
                "api_key",
                "The secret that holds the Mailgun API key",
                "MAILGUN_API_KEY",
            ),
            Ask::text(
                "to",
                "The address each message goes to",
                "team@example.org",
                Lands::Route,
            ),
        ],
    },
];
