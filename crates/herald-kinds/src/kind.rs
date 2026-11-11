use herald_core::{Answering, Command};
use jiff::SignedDuration;

use crate::{DestinationConfig, Placement};

/// A mistake in a destination's table or a route's options: what to write
/// instead, and the key under the table it is at, where it is at one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    /// The key under the table, such as `interactions` or `lists[0]`.
    pub key: Option<String>,
    /// What is wrong, and what to write instead.
    pub message: String,
}

impl Fault {
    /// `message`, at the table itself.
    #[must_use]
    pub fn here(message: impl Into<String>) -> Self {
        Self {
            key: None,
            message: message.into(),
        }
    }

    /// `message`, at `key` under the table.
    #[must_use]
    pub fn at(key: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            key: Some(key.into()),
            message: message.into(),
        }
    }
}

/// What a route's options are checked beside: the route's own say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placing<'a> {
    /// The destination's name, as `[destinations.<name>]` writes it.
    pub destination: &'a str,
    /// Whether the route mentions its subscribers.
    pub subscribers: bool,
}

impl Placing<'_> {
    /// Whether the destination, answering as `answering`, answers
    /// `subscribe`, which a route offering subscriptions needs.
    ///
    /// # Errors
    ///
    /// What to give the destination, where it does not.
    pub fn answers_subscribing(self, answering: Option<Answering>) -> Result<(), String> {
        if answering.is_some_and(|answering| answering.takes(Command::Subscribe)) {
            Ok(())
        } else {
            Err(format!(
                "subscribing is answered through the destination's interactions; add \"{}\" to [destinations.{}.interactions] commands",
                Command::Subscribe.name(),
                self.destination
            ))
        }
    }
}

/// One kind of destination: the options its table reads into and a route's
/// options for it, how each is checked, and how the schema names them.
pub trait DestinationKind: 'static {
    /// The kind's name, as `[destinations.<name>] kind` writes it.
    const NAME: &str;
    /// What the kind is, as the schema describes a destination of it.
    const DESCRIPTION: &str;
    /// The kind, as a route's error names it.
    const LABEL: &str;
    /// The schema's name for a route to it.
    const ROUTE: &str;
    /// What the schema says of a route to it.
    const ROUTE_DESCRIPTION: &str;

    /// The options its table reads into.
    type Options: DestinationConfig + Clone;
    /// A route's own options for it.
    type Route: Placement + Clone;

    /// `options`, checked and completed with the files they name, read
    /// through `read`, with the faults the destination is kept beside.
    ///
    /// # Errors
    ///
    /// The fault that keeps the destination out of the config.
    fn checked(
        options: Self::Options,
        _read: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<(Self::Options, Vec<Fault>), Fault> {
        Ok((options, Vec::new()))
    }

    /// Every fault of `route`, placed through `options`.
    fn placed(options: &Self::Options, route: &Self::Route, placing: Placing<'_>) -> Vec<Fault>;

    /// The fault where a session `options` hold open waits past `total`,
    /// the longest herald waits for any answer.
    fn outlasts(_options: &Self::Options, _total: SignedDuration) -> Option<Fault> {
        None
    }
}

#[cfg(test)]
mod tests {
    use herald_core::{Replies, Wording};

    use super::*;

    fn answering(commands: Vec<Command>) -> Answering {
        Answering {
            commands,
            replies: Replies::default(),
            prereleases: false,
            template: None,
            wording: Wording::default(),
        }
    }

    #[test]
    fn a_fault_is_at_the_table_or_at_a_key_under_it() {
        assert_eq!(
            Fault::here("give one"),
            Fault {
                key: None,
                message: "give one".to_owned()
            }
        );
        assert_eq!(
            Fault::at("lists[0]", "pin it"),
            Fault {
                key: Some("lists[0]".to_owned()),
                message: "pin it".to_owned()
            }
        );
    }

    #[test]
    fn a_route_offers_subscriptions_only_where_its_destination_answers_subscribe() {
        let placing = Placing {
            destination: "chat",
            subscribers: true,
        };
        assert_eq!(
            placing.answers_subscribing(Some(answering(vec![Command::Subscribe]))),
            Ok(())
        );
        let refused = Err(
            "subscribing is answered through the destination's interactions; add \"subscribe\" to [destinations.chat.interactions] commands".to_owned(),
        );
        assert_eq!(
            placing.answers_subscribing(Some(answering(vec![Command::Latest]))),
            refused
        );
        assert_eq!(placing.answers_subscribing(None), refused);
    }
}
