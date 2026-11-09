//! herald's Fediverse destination (ADR-0004).
//!
//! An account the adopter owns posts each announcement as a status through
//! the Mastodon client API, on any server speaking it: Mastodon, `GoToSocial`,
//! Akkoma or Pleroma. The overflow is a chain of replies under it, each
//! status is posted under an idempotency key derived from the never-twice
//! key, and the announcement's link carries its markers.

mod activitypub;
mod actor;
mod markers;
mod mastodon;
mod options;
mod read;
mod request;

pub use activitypub::{
    ActivityPub, ActorOptions, ActorRouteOptions, Approve, Asked, Audience, FIRST, FollowerCaps,
    Mode, Policy, SharedList, accept, actor_document, actor_profile, asked, blocked_domains,
    create, follow_id, note, webfinger,
};
pub use actor::{
    ActorError, ActorKey, Found, Inboxes, activity_json, actor_key, actor_of, confirms, document,
    inboxes, key_request, origin, same_origin,
};
pub use markers::MastodonMarkers;
pub use mastodon::{LIMIT, Mastodon, profile, shortest};
pub use options::{DestinationOptions, RouteOptions, Visibility};
pub use request::Refusal;
