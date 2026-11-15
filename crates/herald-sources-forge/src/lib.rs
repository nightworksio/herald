//! herald's forge sources (ADR-0003): a release on GitHub, GitLab or
//! Forgejo becomes an [`Event`](herald_core::Event), parsed without I/O.
//!
//! Each forge's release event, as a webhook or as its CI's event, announces
//! a published release only, and a draft never announces. Polling a forge's
//! releases API is described as requests the flow sends and responses it
//! hands back.

mod delivered;
mod events;
mod forge;
mod forgejo;
mod github;
mod gitlab;
mod graphql;
mod kind;
mod members;
mod options;
mod owner;
mod polled;
#[cfg(test)]
mod scripted;
mod tags;
mod wire;

pub use forge::{Forge, Origin, Repository, Token};
pub use herald_kinds::Hooked;
pub use kind::{ForgeSource, ForgejoKind, GitHubKind, GitLabKind};
pub use options::{
    GRAPHQL_BATCH, GRAPHQL_BATCH_ALLOWED, KEEP_MEMBERS, KEY, MOST_LISTED, MOST_MEMBERS,
    MOST_MEMBERS_ALLOWED, Members, Override, Signals, SourceOptions, TAG_WINDOW,
    TAG_WINDOW_ALLOWED, Transport, Values,
};
pub use polled::tag_event_of;
