//! herald's Microsoft Teams destination (ADR-0093).
//!
//! A Workflows webhook, the URL of a workflow whose trigger is "When a
//! Teams webhook request is received", posts each announcement as an
//! Adaptive Card: its title as a large bold text block, its body in card
//! Markdown, and each of its kind's links as a button. The URL is the
//! secret, and the only origin herald reaches with it. A webhook only
//! posts: it cannot thread, pin, edit, mention or read its posts back.

mod config;
mod first_run;
mod options;
mod reach;
mod request;
mod teams;

pub use config::{KIND, TeamsKind};
pub use options::{DestinationOptions, RouteOptions};
pub use teams::{MARKUP, MESSAGE, Teams, UNCHECKED, profile};
