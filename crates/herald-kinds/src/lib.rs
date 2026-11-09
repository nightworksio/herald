//! What a kind of destination, source or store is to herald (ADR-0001): the
//! traits each kind's crate implements, so flows, config and composition
//! reach every kind through them and name none.

mod destination;
mod reach;

pub use destination::{Carried, Destination, DestinationConfig};
pub use reach::{Posts, Reach, Reached, Signing, Unread, same};
