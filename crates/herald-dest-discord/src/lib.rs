//! herald's Discord destination (ADR-0004).
//!
//! A bot posts into announcement, text and forum channels, crossposts, opens
//! a thread per announcement, posts the overflow into it, and pins it in
//! place of the route's earlier pin. A webhook
//! posts into a channel or a forum and does nothing else. Each step is a
//! request built from the announcement and the steps already landed, and
//! each response is read back into the facts the ledger records.

mod config;
mod discord;
mod gateway;
mod held;
mod interact;
mod interactions;
mod look;
mod markers;
mod options;
mod reach;
mod read;
mod request;
pub mod setup;
mod unpin;

pub use config::{DiscordKind, KIND, SCHEME};
pub use discord::{Credential, Discord, MARKUP, profile};
pub use gateway::{Effect, GATEWAY, Gateway};
pub use interact::{
    Interacting, MESSAGE, Received, Replying, Unread, choices, deferred, listed, made, pong,
    subscribe_button, took,
};
pub use interactions::{Interactions, SESSION_DOMAIN, Subscribe, Via};
pub use look::{ArchiveAfter, Colour, Colours, Look};
pub use markers::DiscordMarkers;
pub use options::{DestinationOptions, RouteOptions};
pub use request::Refusal;
