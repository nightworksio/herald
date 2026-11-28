//! herald's Microsoft Teams destination (ADR-0093).
//!
//! It posts each announcement as an Adaptive Card: its title as a large
//! bold text block, its body in card Markdown, and each of its kind's
//! links as a button. A Workflows webhook, the URL of a workflow whose
//! trigger is "When a Teams webhook request is received", only posts: the
//! URL is the secret, and the only origin herald reaches with it. A bot
//! signs in with its app registration's client secret for a token held in
//! memory alone, then posts to a channel or chat its app is installed in,
//! edits and deletes its posts, and, where a route says so, replies in
//! their threads. What Teams posts to the bot at herald's endpoint is taken
//! only on the bearer token the Bot Framework vouches for it with.

mod bot;
mod card;
mod cloud;
mod config;
mod first_run;
mod interact;
mod interactions;
mod options;
mod reach;
mod request;
mod teams;

pub use bot::{BOT_MESSAGE, Bot};
pub use cloud::{Cloud, Hosts};
pub use config::{KIND, TeamsKind};
pub use interact::{ACTIVITY, CHANNEL, Interacting, Reply};
pub use interactions::{Interactions, Via};
pub use options::{DestinationOptions, RouteOptions};
pub use teams::{MARKUP, MESSAGE, Teams, UNCHECKED, bot_profile, profile};
