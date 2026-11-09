//! herald's Matrix destination (ADR-0004).
//!
//! A bot account posts each announcement into a room as an `m.room.message`
//! with a plain body and sanitised HTML, under a transaction id derived from
//! the never-twice key, so a retried request is applied once by the
//! homeserver itself. The overflow joins the announcement's thread, the
//! route's subscribers are mentioned through `m.mentions`, the newest
//! announcement is pinned through the room's pinned events, and each post
//! carries its markers in a namespaced field of its content.

mod config;
mod held;
mod interact;
mod interactions;
mod markers;
mod matrix;
mod options;
mod reach;
mod read;
mod request;
pub mod setup;

pub use config::{KIND, MatrixKind};
pub use interact::{Interacting, Replying, Synced, Unread, took};
pub use interactions::{Interactions, SYNC_WAIT, Via};
pub use markers::MatrixMarkers;
pub use matrix::{MARKUP, Matrix, profile};
pub use options::{DestinationOptions, RouteOptions};
pub use request::Refusal;
