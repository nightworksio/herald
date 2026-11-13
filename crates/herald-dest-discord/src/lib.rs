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
mod probe;
mod reach;
mod read;
mod request;
mod setup;
mod unpin;

pub use config::{DiscordKind, KIND, SCHEME};
pub use discord::{Discord, MARKUP, profile};
pub use gateway::{Effect, Gateway};
pub use herald_ports::Credential;
pub use interact::{
    Interacting, MESSAGE, Received, Replying, UnreadInteraction, accepted, choices, deferred,
    listed, made, subscribe_button,
};
pub use interactions::{Interactions, SESSION_DOMAIN, Subscribe, Via};
pub use look::{ArchiveAfter, Colour, Colours, Look};
pub use markers::DiscordMarkers;
pub use options::{DestinationOptions, RouteOptions};
