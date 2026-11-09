//! herald's Slack destination (ADR-0004).
//!
//! A bot posts each announcement with `chat.postMessage` in Block Kit: a
//! header with the title, a section in `mrkdwn`, its link and its mentions,
//! beside a plain-text fallback, with link unfurling off. The overflow
//! replies in the announcement's thread, the route's subscribers are
//! mentioned as `<@U…>`, the newest announcement is pinned, and each post
//! carries its markers in its message metadata (ADR-0011). An incoming
//! webhook only posts.

mod config;
mod domain;
mod held;
mod interact;
mod interactions;
mod markers;
mod options;
mod reach;
mod read;
mod request;
mod slack;
mod socket;
mod web_api;

pub use config::{KIND, SlackKind};
pub use domain::Domain;
pub use interact::{
    Interacting, Received, Replying, SECTION, SUBSCRIBE, Unread, subscribe_block, took,
};
pub use interactions::{Interactions, Subscribe, Via};
pub use markers::SlackMarkers;
pub use options::{DestinationOptions, RouteOptions};
pub use request::Refusal;
pub use slack::{Credential, Slack, profile};
pub use socket::{Enveloped, opened};
