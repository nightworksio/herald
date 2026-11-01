//! herald's forge sources (ADR-0003): a release on a forge becomes an
//! [`Event`](herald_core::Event), parsed without I/O.
//!
//! GitHub's `release` event, as a webhook or as GitHub Actions' event file,
//! announces on `published` only, and a draft never announces. Polling the
//! releases API is described as requests the flow sends and responses it
//! hands back.

mod error;
mod github;
mod options;

pub use error::ForgeError;
pub use github::{GitHub, Polled, Repository};
pub use options::SourceOptions;
