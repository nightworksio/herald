use std::fmt;
use std::future::Future;

use herald_core::{Cursor, DestinationName, Entry, Key, Schedule, SourceName, Subscription};

/// The version a store keeps beside a record, for compare-and-set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(pub u64);

/// A record and its version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Versioned<T> {
    /// The record.
    pub value: T,
    /// Its version.
    pub version: Version,
}

/// What a compare-and-set did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written {
    /// It wrote, and the record now has this version.
    Written(Version),
    /// The record was not at the expected version, and nothing was written.
    Conflict,
}

/// What a store keeps, for doctor to compare with what a config needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Capabilities {
    /// The ledger.
    pub ledger: bool,
    /// Cursors.
    pub cursors: bool,
    /// Schedules.
    pub schedules: bool,
    /// Subscriptions.
    pub subscriptions: bool,
    /// Compare-and-set that holds when runs write at the same time.
    pub concurrent: bool,
}

/// Why a store could not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The store could not be reached.
    Unavailable {
        /// What went wrong.
        reason: String,
    },
    /// A record could not be read back.
    Corrupt {
        /// What went wrong.
        reason: String,
    },
    /// The store does not keep this kind of record.
    Unsupported {
        /// What it does not keep.
        what: &'static str,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable { reason } => write!(f, "the store could not be reached: {reason}"),
            Self::Corrupt { reason } => {
                write!(f, "a record in the store could not be read: {reason}")
            }
            Self::Unsupported { what } => {
                write!(f, "this store keeps no {what}; choose a store that does")
            }
        }
    }
}

/// The ledger, cursors, schedules and subscriptions (ADR-0005).
///
/// Entries and cursors are written with compare-and-set. Schedules and
/// subscriptions are sets, and adding or removing one twice is the same as
/// once.
pub trait Store: Sync {
    /// What the store keeps.
    fn capabilities(&self) -> Capabilities;

    /// The ledger's entry for `key`.
    fn entry(
        &self,
        key: &Key,
    ) -> impl Future<Output = Result<Option<Versioned<Entry>>, StoreError>> + Send;

    /// Writes `entry` if the stored one is at `expected`, or absent when
    /// `expected` is `None`.
    fn put_entry(
        &self,
        entry: &Entry,
        expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send;

    /// The cursor of `source`.
    fn cursor(
        &self,
        source: &SourceName,
    ) -> impl Future<Output = Result<Option<Versioned<Cursor>>, StoreError>> + Send;

    /// Writes the cursor of `source` if the stored one is at `expected`, or
    /// absent when `expected` is `None`.
    fn put_cursor(
        &self,
        source: &SourceName,
        cursor: &Cursor,
        expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send;

    /// Every schedule.
    fn schedules(&self) -> impl Future<Output = Result<Vec<Schedule>, StoreError>> + Send;

    /// Adds `add` and removes `remove`.
    fn put_schedules(
        &self,
        add: &[Schedule],
        remove: &[Schedule],
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// The subscribers to `project` on `destination`.
    fn subscribers(
        &self,
        destination: &DestinationName,
        project: &str,
    ) -> impl Future<Output = Result<Vec<String>, StoreError>> + Send;

    /// Adds `subscription`.
    fn subscribe(
        &self,
        subscription: &Subscription,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Removes `subscription`.
    fn unsubscribe(
        &self,
        subscription: &Subscription,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_store_error_reads_as_a_sentence() {
        assert_eq!(
            StoreError::Unavailable {
                reason: "down".to_owned()
            }
            .to_string(),
            "the store could not be reached: down"
        );
        assert_eq!(
            StoreError::Corrupt {
                reason: "bad json".to_owned()
            }
            .to_string(),
            "a record in the store could not be read: bad json"
        );
        assert_eq!(
            StoreError::Unsupported { what: "cursors" }.to_string(),
            "this store keeps no cursors; choose a store that does"
        );
    }
}
