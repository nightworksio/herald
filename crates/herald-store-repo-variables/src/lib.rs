//! herald's records kept as the repository's Actions variables, through
//! GitHub's API (ADR-0005).
//!
//! Each record is one variable, named for its path. A write holds the lock
//! variable, whose creation GitHub refuses while it exists: that refusal is
//! the compare-and-set. A lock older than its lease, by GitHub's own clock,
//! is taken as abandoned. Writing variables needs a token with the
//! repository's Variables permission, which the job's own token lacks.

mod name;
mod options;
mod variables;

pub use options::StoreOptions;
pub use variables::{RepoVariables, RepoVariablesStore, VariableRecords};
