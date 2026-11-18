//! What the instance published, and what its peers handed over to, read
//! through an erased store.

use herald_core::{DestinationName, Feed, PeerKeys, PublishedKeys};

use super::DynStore;
use crate::erased::Erased;
use crate::store::{Published, StoreError, Version, Versioned, Written};

impl Published for Erased<'_, dyn DynStore + '_> {
    async fn published_keys(&self) -> Result<Option<Versioned<PublishedKeys>>, StoreError> {
        self.0.published_keys_boxed().await
    }

    async fn put_published_keys(
        &self,
        keys: &PublishedKeys,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0.put_published_keys_boxed(keys, expected).await
    }

    async fn feed(
        &self,
        destination: &DestinationName,
    ) -> Result<Option<Versioned<Feed>>, StoreError> {
        self.0.feed_boxed(destination).await
    }

    async fn put_feed(
        &self,
        destination: &DestinationName,
        feed: &Feed,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0.put_feed_boxed(destination, feed, expected).await
    }

    async fn peer_keys(&self, peer: &str) -> Result<Option<Versioned<PeerKeys>>, StoreError> {
        self.0.peer_keys_boxed(peer).await
    }

    async fn put_peer_keys(
        &self,
        peer: &str,
        keys: &PeerKeys,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0.put_peer_keys_boxed(peer, keys, expected).await
    }
}
