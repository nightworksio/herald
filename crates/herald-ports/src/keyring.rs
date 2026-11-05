use std::future::Future;

use jiff::Timestamp;

use crate::store::StoreError;

/// A key read from the document its id names, and the actor owning it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownKey {
    /// The actor that owns the key.
    pub owner: String,
    /// The key, as a PEM.
    pub pem: String,
    /// When its document was read.
    pub fetched: Timestamp,
}

/// The keys inbound activities were checked with, kept by their id so the
/// next activity signed with one is checked without reading its document
/// again.
pub trait Keyring: Sync {
    /// The key `id` names, where it is kept.
    fn known(&self, id: &str) -> impl Future<Output = Result<Option<KnownKey>, StoreError>> + Send;

    /// Keeps `key` as the key `id` names.
    fn know(&self, id: &str, key: &KnownKey)
    -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Forgets every key read before `before`.
    fn forget_before(
        &self,
        before: Timestamp,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}
