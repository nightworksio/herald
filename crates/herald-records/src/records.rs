use std::future::Future;

use herald_ports::StoreError;

/// A record's bytes as a backend holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blob {
    /// What a write must name to replace these bytes.
    pub token: String,
    /// The bytes.
    pub bytes: Vec<u8>,
}

/// What a write or a removal came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrote {
    /// It took effect.
    Written,
    /// The record changed since it was read, or exists where it was to be
    /// new, and nothing changed.
    Conflict,
}

/// A backend keeping one record per path.
pub trait Records: Sync {
    /// The record at `path`, if it holds one.
    fn read(&self, path: &str) -> impl Future<Output = Result<Option<Blob>, StoreError>> + Send;

    /// The paths of the records in the directory `directory`.
    fn list(&self, directory: &str)
    -> impl Future<Output = Result<Vec<String>, StoreError>> + Send;

    /// Writes `bytes` at `path` where the record there is still the one
    /// `replacing` names, or where there is none when `replacing` is `None`.
    fn write(
        &self,
        path: &str,
        bytes: &[u8],
        replacing: Option<&str>,
    ) -> impl Future<Output = Result<Wrote, StoreError>> + Send;

    /// Removes the record at `path` where it is still the one `replacing`
    /// names. A record already gone counts as removed.
    fn remove(
        &self,
        path: &str,
        replacing: &str,
    ) -> impl Future<Output = Result<Wrote, StoreError>> + Send;
}
