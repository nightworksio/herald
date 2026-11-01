//! herald's `Store` kept one record per path (ADR-0001, ADR-0005), over any
//! backend that replaces a record only as it was read.
//!
//! A backend implements [`Records`]: it reads a record's bytes with the token
//! a write must name to replace them, lists a directory, and writes or
//! removes a record only where it still holds what that token names. [`Kept`]
//! is the `Store` over it: the paths each kind of record is kept at, the
//! version kept beside each record, and the compare-and-set built on the
//! backend's.

mod kept;
mod memory;
mod paths;
mod records;

pub use kept::Kept;
pub use memory::Memory;
pub use records::{Blob, Records, Wrote};
