//! herald's records kept one file per record on a `herald-state` branch of
//! a Forgejo repository, through Forgejo's contents API (ADR-0090).
//!
//! A write names the blob it replaces. Forgejo checks it against the
//! branch's head and pushes its commit onto that head without force, so a
//! write is refused once the file changed, and once the branch moved since
//! the check: that refusal is the compare-and-set.

mod kind;

pub use kind::ForgejoBranchKind;

mod contents;
mod options;
mod store;

pub use options::StoreOptions;
pub use store::{BranchRecords, ForgejoBranch, ForgejoBranchStore};
