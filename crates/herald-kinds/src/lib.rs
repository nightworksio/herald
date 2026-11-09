//! What a kind of destination, source or store is to herald (ADR-0001): the
//! traits each kind's crate implements, so flows, config and composition
//! reach every kind through them and name none.

mod destination;
mod interacts;
mod kind;
mod placement;
mod reach;
mod session;

pub use destination::{Carried, Destination, DestinationConfig, UNREAD};
pub use interacts::{Endpoint, Interacted, Interacts, Post, Read, Replying, Said, optional};
pub use kind::{DestinationKind, Fault, Placing};
pub use placement::{Placed, Placement};
pub use reach::{Posts, Reach, Reached, Signing, Unread, same};
pub use session::{Connect, Effect, Session};
