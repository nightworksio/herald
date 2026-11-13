//! What a kind of destination, source or store is to herald (ADR-0001): the
//! traits each kind's crate implements, so flows, config and composition
//! reach every kind through them and name none.

mod destination;
mod federates;
mod interacts;
mod itself;
mod kind;
mod opening;
mod placement;
mod reach;
mod session;
mod setup;
mod source;
mod store;

pub use destination::{Carried, Destination, DestinationConfig, UNREAD};
pub use federates::{Activity, Federates, Inboxes};
pub use interacts::{Endpoint, Interacted, Interacts, Post, Read, Replying, Said, optional};
pub use itself::Itself;
pub use kind::{DestinationKind, Fault, Placing};
pub use opening::{GITHUB_REPOSITORY, GITHUB_TOKEN, KvAt, Local, Opened, Opening, SqlAt, unopened};
pub use placement::{Placed, Placement};
pub use reach::{Posts, Probe, Reach, Reached, Signing, UnreadPosts};
pub use session::{Connect, Effect, Session};
pub use setup::{Made, Served, Setup, SetupNeeds, SetupStep, SetupTask};
pub use source::{Hooked, Poll, Source, SourceConfig, SourceKind, UNHOOKED, Unpolled};
pub use store::{ConfiguredStore, Offer, StoreConfig, StoreKind};
