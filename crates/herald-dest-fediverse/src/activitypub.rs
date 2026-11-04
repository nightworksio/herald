//! herald's own ActivityPub actor (ADR-0004, decision 7): each project
//! announced this way is an actor on herald's domain. An announcement is a
//! `Create` of a `Note`, delivered to each follower's inbox, signed with an
//! RSA-SHA256 HTTP signature as Mastodon verifies it.

mod deliver;
mod documents;
mod options;

pub use deliver::{ActivityPub, actor_profile};
pub use documents::{actor_document, create, note, webfinger};
pub use options::{ActorOptions, ActorRouteOptions};
