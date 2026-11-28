//! How a destination of a kind is first set up, without I/O (ADR-0024
//! decision 5, ADR-0091): the ways it is reached, each with the questions
//! `herald init` asks and where each answer is written. The kind answers
//! before any table of it exists.

/// A destination kind a first run may choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirstRun {
    /// The kind's name, as `kind` writes it.
    pub kind: &'static str,
    /// What the kind is.
    pub described: &'static str,
    /// The ways it is first reached, the recommended first.
    pub variants: &'static [Variant],
}

/// One way a destination of a kind is reached, such as a webhook or a bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Variant {
    /// Its name, as `herald init` offers it.
    pub name: &'static str,
    /// What it is and what it needs, on one line.
    pub described: &'static str,
    /// Whether it reads its own posts back, so the markers store keeps
    /// "sent once" for it.
    pub reads_back: bool,
    /// The questions it asks, in order.
    pub asks: &'static [Ask],
}

impl Variant {
    /// The keys `answers`, one per question in its order, write: each
    /// answer the config takes where its key is absent left out.
    #[must_use]
    pub fn written(&self, answers: &[String]) -> Vec<Key> {
        self.asks
            .iter()
            .zip(answers)
            .filter(|(ask, answer)| ask.answer.written(answer))
            .map(|(ask, answer)| Key {
                lands: ask.lands,
                key: ask.key,
                value: match ask.answer {
                    Answer::Secret { .. } => Written::Secret(answer.clone()),
                    Answer::Text | Answer::Choice { .. } | Answer::Fixed { .. } => {
                        Written::Text(answer.clone())
                    }
                },
            })
            .collect()
    }
}

/// One key an answer writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    /// The table it is written to.
    pub lands: Lands,
    /// The key.
    pub key: &'static str,
    /// What it is written as.
    pub value: Written,
}

/// What a key is written as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Written {
    /// `{ secret = "<name>" }`: the secret by its name.
    Secret(String),
    /// The text as given.
    Text(String),
}

/// One question of a variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ask {
    /// The key its answer is written as.
    pub key: &'static str,
    /// What it asks.
    pub asked: &'static str,
    /// What an answer looks like, shown with the question.
    pub example: &'static str,
    /// What it takes.
    pub answer: Answer,
    /// The table its answer is written to.
    pub lands: Lands,
}

impl Ask {
    /// A question asking for the name of the secret that holds the
    /// destination's credential, `default` the recommended name.
    #[must_use]
    pub const fn secret(key: &'static str, asked: &'static str, default: &'static str) -> Self {
        Self {
            key,
            asked,
            example: default,
            answer: Answer::Secret { default },
            lands: Lands::Destination,
        }
    }

    /// A question asking for text written as given, such as `example`.
    #[must_use]
    pub const fn text(
        key: &'static str,
        asked: &'static str,
        example: &'static str,
        lands: Lands,
    ) -> Self {
        Self {
            key,
            asked,
            example,
            answer: Answer::Text,
            lands,
        }
    }
}

/// What a question takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// The name of the secret that holds the credential, `default` where
    /// none is given; the value is never asked.
    Secret {
        /// The recommended name.
        default: &'static str,
    },
    /// Text written as given, such as an id; none is a refusal.
    Text,
    /// One of `choices`, the first where none is given, which the config
    /// takes where the key is absent, so it is then not written.
    Choice {
        /// The values it takes, the first the one taken where absent.
        choices: &'static [&'static str],
    },
    /// `value`, always written and never asked: what the variant is, such
    /// as the way its messages leave.
    Fixed {
        /// What is written.
        value: &'static str,
    },
}

/// The table an answer is written to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lands {
    /// The destination's own table.
    Destination,
    /// Each route to it.
    Route,
}

impl Answer {
    /// The answer taken where none is given, where there is one.
    #[must_use]
    pub const fn default(self) -> Option<&'static str> {
        match self {
            Self::Secret { default } | Self::Fixed { value: default } => Some(default),
            Self::Choice {
                choices: [first, ..],
            } => Some(first),
            Self::Text | Self::Choice { choices: [] } => None,
        }
    }

    /// Whether `answer` is written: a choice the config takes where its key
    /// is absent is not.
    #[must_use]
    pub fn written(self, answer: &str) -> bool {
        match self {
            Self::Choice { choices } => choices.first().is_none_or(|first| *first != answer),
            Self::Secret { .. } | Self::Text | Self::Fixed { .. } => true,
        }
    }

    /// `answer` checked: a secret's name as herald names secrets, text not
    /// empty, a choice among its choices.
    ///
    /// # Errors
    ///
    /// Why the answer is refused, naming what is taken.
    pub fn checked(self, answer: &str) -> Result<(), String> {
        match self {
            Self::Secret { .. } => {
                let named = !answer.is_empty()
                    && answer.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                    })
                    && !answer.starts_with(|first: char| first.is_ascii_digit());
                if named {
                    Ok(())
                } else {
                    Err("name the secret in capitals, digits and underscores, such as HERALD_SLACK_WEBHOOK; its value is never asked".to_owned())
                }
            }
            Self::Text if answer.is_empty() => Err("give an answer".to_owned()),
            Self::Text => Ok(()),
            Self::Choice { choices } if choices.contains(&answer) => Ok(()),
            Self::Choice { choices } => Err(format!("answer one of {}", choices.join(", "))),
            Self::Fixed { value } if answer == value => Ok(()),
            Self::Fixed { value } => Err(format!("it is always {value}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEBHOOK: Answer = Answer::Secret {
        default: "HERALD_SLACK_WEBHOOK",
    };
    const DOMAIN: Answer = Answer::Choice {
        choices: &["slack.com", "slack-gov.com"],
    };

    #[test]
    fn a_secret_defaults_to_its_name_and_is_always_written() {
        assert_eq!(WEBHOOK.default(), Some("HERALD_SLACK_WEBHOOK"));
        assert!(WEBHOOK.written("HERALD_SLACK_WEBHOOK"));
    }

    #[test]
    fn a_secret_is_named_in_capitals_digits_and_underscores() {
        assert_eq!(WEBHOOK.checked("HERALD_TEAM_2"), Ok(()));
        for refused in [
            "",
            "herald_slack",
            "HERALD-SLACK",
            "2HERALD",
            "https://hooks.slack.com/x",
        ] {
            let why = WEBHOOK.checked(refused).unwrap_err();
            assert!(why.contains("its value is never asked"), "{refused}: {why}");
        }
    }

    #[test]
    fn text_takes_anything_but_nothing() {
        assert_eq!(Answer::Text.default(), None);
        assert_eq!(Answer::Text.checked("C0123456789"), Ok(()));
        assert_eq!(Answer::Text.checked(""), Err("give an answer".to_owned()));
        assert!(Answer::Text.written("C0123456789"));
    }

    #[test]
    fn a_choice_defaults_to_its_first_which_is_not_written() {
        assert_eq!(DOMAIN.default(), Some("slack.com"));
        assert!(!DOMAIN.written("slack.com"));
        assert!(DOMAIN.written("slack-gov.com"));
        assert_eq!(DOMAIN.checked("slack-gov.com"), Ok(()));
        assert_eq!(
            DOMAIN.checked("example.com"),
            Err("answer one of slack.com, slack-gov.com".to_owned())
        );
    }

    #[test]
    fn a_variant_writes_a_secret_by_its_name_and_leaves_a_default_choice_out() {
        let variant = Variant {
            name: "bot",
            described: "a bot",
            reads_back: true,
            asks: &[
                Ask {
                    key: "token",
                    asked: "The secret",
                    example: "HERALD_SLACK_TOKEN",
                    answer: WEBHOOK,
                    lands: Lands::Destination,
                },
                Ask {
                    key: "domain",
                    asked: "Which Slack",
                    example: "slack.com",
                    answer: DOMAIN,
                    lands: Lands::Destination,
                },
                Ask {
                    key: "channel",
                    asked: "The channel",
                    example: "C0123456789",
                    answer: Answer::Text,
                    lands: Lands::Route,
                },
            ],
        };
        let answers = |domain: &str| ["HERALD_TEAM", domain, "C0123456789"].map(str::to_owned);
        let secret = Key {
            lands: Lands::Destination,
            key: "token",
            value: Written::Secret("HERALD_TEAM".to_owned()),
        };
        let channel = Key {
            lands: Lands::Route,
            key: "channel",
            value: Written::Text("C0123456789".to_owned()),
        };
        assert_eq!(
            variant.written(&answers("slack.com")),
            [secret.clone(), channel.clone()]
        );
        let gov = Key {
            lands: Lands::Destination,
            key: "domain",
            value: Written::Text("slack-gov.com".to_owned()),
        };
        assert_eq!(
            variant.written(&answers("slack-gov.com")),
            [secret, gov, channel]
        );
    }

    #[test]
    fn a_secret_question_lands_on_the_destination_with_its_default_as_example() {
        let built = std::hint::black_box(Ask::secret)("token", "The secret", "HERALD_SLACK_TOKEN");
        assert_eq!(
            (built.key, built.asked, built.example),
            ("token", "The secret", "HERALD_SLACK_TOKEN")
        );
        assert_eq!(
            built.answer,
            Answer::Secret {
                default: "HERALD_SLACK_TOKEN"
            }
        );
        assert_eq!(built.lands, Lands::Destination);
    }

    #[test]
    fn a_text_question_lands_where_it_is_told() {
        let text = std::hint::black_box(Ask::text);
        for lands in [Lands::Destination, Lands::Route] {
            let built = text("channel", "The channel", "C0123456789", lands);
            assert_eq!(
                (built.key, built.asked, built.example),
                ("channel", "The channel", "C0123456789")
            );
            assert_eq!(built.answer, Answer::Text);
            assert_eq!(built.lands, lands);
        }
    }

    #[test]
    fn a_fixed_answer_is_its_value_alone_and_always_written() {
        let fixed = Answer::Fixed { value: "ses" };
        assert_eq!(fixed.default(), Some("ses"));
        assert!(fixed.written("ses"));
        assert_eq!(fixed.checked("ses"), Ok(()));
        assert_eq!(fixed.checked("smtp"), Err("it is always ses".to_owned()));
    }

    #[test]
    fn a_choice_of_none_has_no_default_and_is_written() {
        let none = Answer::Choice { choices: &[] };
        assert_eq!(none.default(), None);
        assert!(none.written("anything"));
    }
}
