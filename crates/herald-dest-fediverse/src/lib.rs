//! herald's Fediverse destination (ADR-0004).
//!
//! An account the adopter owns posts each announcement as a status through
//! the Mastodon client API, on any server speaking it: Mastodon, `GoToSocial`,
//! Akkoma or Pleroma. The overflow is a chain of replies under it, each
//! status is posted under an idempotency key derived from the never-twice
//! key, and the announcement's link carries its markers.

mod markers;
mod mastodon;
mod options;
mod read;
mod request;

pub use markers::MastodonMarkers;
pub use mastodon::{LIMIT, Mastodon, profile, shortest};
pub use options::{DestinationOptions, RouteOptions, Visibility};
pub use request::Refusal;
