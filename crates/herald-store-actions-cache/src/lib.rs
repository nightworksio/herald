//! herald's records kept in generations of the GitHub Actions cache
//! (ADR-0005).
//!
//! Every record lives in one document, and each write saves the next
//! generation of it as a new cache entry. The cache service refuses an entry
//! that exists, so of two runs writing the same generation one is refused:
//! that refusal is the compare-and-set. The newest generation is the newest
//! entry the key's prefix finds. The cache is scoped to the ref a run is on,
//! and drops what no run reads for a week.

mod cache;
mod options;
mod service;

pub use cache::{ActionsCache, ActionsCacheStore, Blobs, CacheRecords};
pub use options::StoreOptions;
