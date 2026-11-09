//! What a kind of destination, source or store is to herald (ADR-0001): the
//! traits each kind's crate implements, so flows, config and composition
//! reach every kind through them and name none.

mod reach;

pub use reach::{Posts, Reach, Reached, Signing, Unread, same};
