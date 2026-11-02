//! herald's records kept one file per record on a `herald-state` branch of
//! the repository, through GitHub's contents API (ADR-0005).
//!
//! A write names the blob it replaces, and GitHub refuses it when the file
//! changed since it was read: that refusal is the compare-and-set.

mod contents;
mod options;
mod store;

pub use options::StoreOptions;
pub use store::{BranchRecords, GitBranch, GitBranchStore};
