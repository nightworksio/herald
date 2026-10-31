use std::fmt;
use std::future::Future;

use herald_core::{Entry, Key, Marker};

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

/// The markers a destination's recent posts carry (ADR-0005, ADR-0011).
pub trait Markers: Sync {
    /// The ledger entry for `key` made by herald's own post carrying
    /// `marker`, the MAC of `key`: which steps landed, read from that post. A
    /// post by anyone else is never read as herald's.
    fn entry(
        &self,
        key: &Key,
        marker: &Marker,
    ) -> impl Future<Output = Result<Option<Entry>, MarkersError>> + Send;
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
