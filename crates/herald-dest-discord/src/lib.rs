//! herald's Discord destination (ADR-0004).
//!
//! A bot posts into announcement, text and forum channels, crossposts, opens
//! a thread per announcement, posts the overflow into it and pins. A webhook
//! posts into a channel or a forum and does nothing else. Each step is a
//! request built from the announcement and the steps already landed, and
//! each response is read back into the facts the ledger records.

mod discord;
mod markers;
mod options;
mod read;
mod request;

pub use discord::{Credential, Discord};
pub use markers::DiscordMarkers;
pub use options::{DestinationOptions, RouteOptions};
pub use request::Refusal;
