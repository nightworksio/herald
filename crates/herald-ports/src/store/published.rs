//! What the instance last published to its peers: its key set and the
//! sequence it published it under (ADR-0044), and each feed's envelopes;
//! and the key set each peer last handed over to (ADR-0049).

use std::future::Future;

use herald_core::{DestinationName, Feed, PeerKeys, PublishedKeys};

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

    /// What the feed of `destination` lists, where it lists anything.
    fn feed(
        &self,
        destination: &DestinationName,
    ) -> impl Future<Output = Result<Option<Versioned<Feed>>, StoreError>> + Send;

    /// Writes the feed of `destination` as `feed` if the stored one is at
    /// `expected`, or absent when `expected` is `None`.
    fn put_feed(
        &self,
        destination: &DestinationName,
        feed: &Feed,
        expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send;

    /// The key set the peer `peer` last handed over to, where one was kept.
    fn peer_keys(
        &self,
        peer: &str,
    ) -> impl Future<Output = Result<Option<Versioned<PeerKeys>>, StoreError>> + Send;

    /// Writes the key set of the peer `peer` as `keys` if the stored one is
    /// at `expected`, or absent when `expected` is `None`.
    fn put_peer_keys(
        &self,
        peer: &str,
        keys: &PeerKeys,
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

    fn feed(
        &self,
        _: &DestinationName,
    ) -> impl Future<Output = Result<Option<Versioned<Feed>>, StoreError>> + Send {
        std::future::ready(Err(super::Held::Published.unsupported()))
    }

    fn put_feed(
        &self,
        _: &DestinationName,
        _: &Feed,
        _: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send {
        std::future::ready(Err(super::Held::Published.unsupported()))
    }

    fn peer_keys(
        &self,
        _: &str,
    ) -> impl Future<Output = Result<Option<Versioned<PeerKeys>>, StoreError>> + Send {
        std::future::ready(Err(super::Held::Published.unsupported()))
    }

    fn put_peer_keys(
        &self,
        _: &str,
        _: &PeerKeys,
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

    async fn feed(
        &self,
        destination: &DestinationName,
    ) -> Result<Option<Versioned<Feed>>, StoreError> {
        match self {
            Some(kept) => kept.feed(destination).await,
            None => super::Unkept.feed(destination).await,
        }
    }

    async fn put_feed(
        &self,
        destination: &DestinationName,
        feed: &Feed,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_feed(destination, feed, expected).await,
            None => super::Unkept.put_feed(destination, feed, expected).await,
        }
    }

    async fn peer_keys(&self, peer: &str) -> Result<Option<Versioned<PeerKeys>>, StoreError> {
        match self {
            Some(kept) => kept.peer_keys(peer).await,
            None => super::Unkept.peer_keys(peer).await,
        }
    }

    async fn put_peer_keys(
        &self,
        peer: &str,
        keys: &PeerKeys,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_peer_keys(peer, keys, expected).await,
            None => super::Unkept.put_peer_keys(peer, keys, expected).await,
        }
    }
}
