//! What herald instances say to each other, with no I/O (ADR-0001,
//! ADR-0043): JSON written one way (RFC 8785), integrity proofs on
//! herald's objects (W3C Data Integrity, `eddsa-jcs-2022`), HTTP message
//! signatures by Ed25519 (RFC 9421) over a body's digest (RFC 9530), and an
//! instance's key set with the rotation statement that replaces it.

pub mod digest;
pub mod jcs;
pub mod keys;
pub mod message;
pub mod multibase;
pub mod proof;
pub mod structured;
