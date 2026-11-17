//! What the instance last published to its peers: its key set and the
//! sequence it published it under (ADR-0044).

use std::future::Future;

use herald_core::PublishedKeys;

use super::{StoreError, Version, Versioned, Written};

/// What the instance published, each record written with compare-and-set.
pub trait Published: Sync {
    /// The key set last published, where one was.
    fn published_keys(
        &self,
    ) -> impl Future<Output = Result<Option<Versioned<PublishedKeys>>, StoreError>> + Send;

    /// Writes `keys` if the stored set is at `expected`, or absent when
    /// `expected` is `None`.
    fn put_published_keys(
        &self,
        keys: &PublishedKeys,
        expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send;
}

impl Published for super::Unkept {
    fn published_keys(
        &self,
    ) -> impl Future<Output = Result<Option<Versioned<PublishedKeys>>, StoreError>> + Send {
        std::future::ready(Err(super::Held::Published.unsupported()))
    }

    fn put_published_keys(
        &self,
        _: &PublishedKeys,
        _: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send {
        std::future::ready(Err(super::Held::Published.unsupported()))
    }
}

impl<T: Published> Published for Option<&T> {
    async fn published_keys(&self) -> Result<Option<Versioned<PublishedKeys>>, StoreError> {
        match self {
            Some(kept) => kept.published_keys().await,
            None => super::Unkept.published_keys().await,
        }
    }

    async fn put_published_keys(
        &self,
        keys: &PublishedKeys,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_published_keys(keys, expected).await,
            None => super::Unkept.put_published_keys(keys, expected).await,
        }
    }
}
