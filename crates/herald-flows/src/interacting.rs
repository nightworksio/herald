//! Each destination that takes commands, with its secrets resolved, and
//! where an answer to one of its commands goes (ADR-0016).

use herald_core::{Answering, Dialect, Measure, Room, Text};
use herald_dest_discord as discord;
use herald_dest_matrix as matrix;
use herald_dest_slack as slack;
use herald_ports::{Http, Request, Secret};

/// A destination that takes commands, with the secrets it needs resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Interacting {
    /// Discord, with the bot token it gives roles and joins the gateway
    /// with.
    Discord(discord::Interacting),
    /// Slack, with the signing secret requests are checked against and the
    /// app-level token Socket Mode is opened with.
    Slack {
        /// How it takes and answers commands.
        interacting: slack::Interacting,
        /// The signing secret.
        signing: Option<Secret>,
        /// The app-level token.
        app: Option<Secret>,
    },
    /// Matrix, with the token the homeserver presents with each transaction.
    Matrix {
        /// How it takes and answers commands.
        interacting: matrix::Interacting,
        /// The homeserver's token.
        homeserver_token: Option<Secret>,
    },
}

/// Where an answer to a command goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Replying {
    /// A Discord interaction's webhook.
    Discord(discord::Replying),
    /// A Slack command's response URL.
    Slack(slack::Replying),
    /// The Matrix room a command was given in.
    Matrix(matrix::Replying),
}

impl Interacting {
    /// The base URL every answer it gives is sent to.
    #[must_use]
    pub fn api(&self) -> &str {
        match self {
            Self::Discord(interacting) => interacting.api(),
            Self::Slack { interacting, .. } => interacting.hooks(),
            Self::Matrix { interacting, .. } => interacting.api(),
        }
    }

    /// The base URL a session herald holds is opened at, where it holds
    /// one beside the answers it sends: Slack's API, where Socket Mode
    /// connections are opened.
    #[must_use]
    pub fn sessions(&self) -> Option<&str> {
        match self {
            Self::Slack {
                interacting,
                app: Some(_),
                ..
            } => Some(interacting.api()),
            _ => None,
        }
    }

    /// How it answers, whatever carries the commands.
    #[must_use]
    pub fn answering(&self) -> Answering {
        match self {
            Self::Discord(interacting) => interacting.answering(),
            Self::Slack { interacting, .. } => interacting.answering(),
            Self::Matrix { interacting, .. } => interacting.answering(),
        }
    }

    /// The dialect its answers are written in, and the room each part of one
    /// fits.
    #[must_use]
    pub fn room(&self) -> (Dialect, Room) {
        let (dialect, measure, most) = match self {
            Self::Discord(_) => (Dialect::Discord, Measure::Chars, discord::MESSAGE),
            Self::Slack { .. } => (Dialect::Slack, Measure::Chars, slack::SECTION),
            Self::Matrix { .. } => {
                let profile = matrix::profile();
                (Dialect::Matrix, profile.measure, profile.limits.followup)
            }
        };
        (
            dialect,
            Room {
                measure,
                first: most,
                rest: most,
            },
        )
    }

    /// The requests that answer the command `replying` names with `parts`,
    /// seen by the asker alone unless `public`; none where `replying` is of
    /// another destination's kind.
    #[must_use]
    pub fn answered(&self, replying: &Replying, parts: &[Text], public: bool) -> Vec<Request> {
        let bodies = || {
            parts
                .iter()
                .map(|part| part.body.clone())
                .collect::<Vec<_>>()
        };
        match (self, replying) {
            (Self::Discord(interacting), Replying::Discord(replying)) => {
                interacting.answered(replying, &bodies(), public)
            }
            (Self::Slack { interacting, .. }, Replying::Slack(replying)) => {
                vec![interacting.answered(replying, &bodies(), public)]
            }
            (Self::Matrix { interacting, .. }, Replying::Matrix(replying)) => {
                interacting.answered(replying, parts, public)
            }
            _ => Vec::new(),
        }
    }

    /// Whether the destination took a request answering a command, or else
    /// why not.
    ///
    /// # Errors
    ///
    /// What the destination said.
    pub(crate) async fn sent<H: Http>(&self, http: &H, request: Request) -> Result<(), String> {
        let response = http
            .send(request)
            .await
            .map_err(|error| error.to_string())?;
        match self {
            Self::Discord(_) => discord::took(&response),
            Self::Slack { .. } => slack::took(&response),
            Self::Matrix { .. } => matrix::took(&response),
        }
    }
}
