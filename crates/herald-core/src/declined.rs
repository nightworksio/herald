use std::fmt;

/// Why a destination declines a step before it asks anything of its
/// platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Declined {
    /// The platform has no such thing to do.
    Cannot {
        /// The platform, by name.
        platform: &'static str,
        /// What it cannot do.
        what: &'static str,
    },
    /// An incoming webhook cannot do it; a bot can.
    WebhookCannot {
        /// The platform, by name.
        platform: &'static str,
        /// What a webhook cannot do.
        what: &'static str,
    },
    /// A bot posts to a channel, and the route names none.
    NoChannel {
        /// The platform, by name.
        platform: &'static str,
    },
    /// The step needs what an earlier step left, which is not recorded.
    Missing(&'static str),
    /// What an earlier step left is recorded, but is not an id of the
    /// platform's own shape, so it addresses nothing herald could send to.
    Malformed(&'static str),
    /// A path segment it would send is empty, `.` or `..`, which names no
    /// one thing, so no request is built from it (ADR-0067).
    Unnamed(&'static str),
}

impl Declined {
    /// `text` as one segment of a URL's path, through the one segment
    /// encoder; declined as `what` where it refuses it.
    ///
    /// # Errors
    ///
    /// Where `text` is empty, `.` or `..`.
    pub fn segment(text: &str, what: &'static str) -> Result<String, Self> {
        crate::segment(text).ok_or(Self::Unnamed(what))
    }
}

impl fmt::Display for Declined {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cannot { platform, what } => write!(f, "{platform} cannot {what}"),
            Self::WebhookCannot { platform, what } => {
                write!(f, "a {platform} webhook cannot {what}; use a bot token")
            }
            Self::NoChannel { platform } => {
                write!(f, "the route names no {platform} channel; set its channel")
            }
            Self::Missing(what) => write!(
                f,
                "{what} is not recorded, so this step cannot be taken yet"
            ),
            Self::Malformed(what) => write!(
                f,
                "the recorded {what} is not an id the platform gives, so this step is not taken"
            ),
            Self::Unnamed(what) => write!(
                f,
                "the {what} is empty or a dot segment, which names nothing in a URL path, so this step is not taken"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_reason_reads_as_a_sentence_naming_the_fix() {
        let platform = "Slack";
        assert_eq!(
            Declined::Cannot {
                platform,
                what: "crosspost"
            }
            .to_string(),
            "Slack cannot crosspost"
        );
        assert_eq!(
            Declined::WebhookCannot {
                platform,
                what: "pin"
            }
            .to_string(),
            "a Slack webhook cannot pin; use a bot token"
        );
        assert_eq!(
            Declined::NoChannel { platform }.to_string(),
            "the route names no Slack channel; set its channel"
        );
        assert_eq!(
            Declined::Missing("the announcement's message").to_string(),
            "the announcement's message is not recorded, so this step cannot be taken yet"
        );
        assert_eq!(
            Declined::segment("..", "Discord channel id"),
            Err(Declined::Unnamed("Discord channel id"))
        );
        assert_eq!(
            Declined::Unnamed("Discord channel id").to_string(),
            "the Discord channel id is empty or a dot segment, which names nothing in a URL path, so this step is not taken"
        );
        assert_eq!(Declined::segment("a/b", "id"), Ok("a%2Fb".to_owned()));
    }
}
