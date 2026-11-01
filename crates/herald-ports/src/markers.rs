use std::fmt;
use std::future::Future;

use herald_core::{Entry, Key};

/// Why a destination's posts could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkersError {
    /// The destination could not be read.
    Unreadable {
        /// What it said.
        reason: String,
    },
}

impl fmt::Display for MarkersError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { reason } => {
                write!(f, "the destination's posts could not be read: {reason}")
            }
        }
    }
}

/// The never-twice keys a destination's recent posts carry (ADR-0005).
pub trait Markers: Sync {
    /// The ledger entry the destination's own posts make for `key`: which
    /// steps landed, read from the post that carries the key.
    fn entry(&self, key: &Key) -> impl Future<Output = Result<Option<Entry>, MarkersError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_error_reads_as_a_sentence() {
        assert_eq!(
            MarkersError::Unreadable {
                reason: "403".to_owned()
            }
            .to_string(),
            "the destination's posts could not be read: 403"
        );
    }
}
