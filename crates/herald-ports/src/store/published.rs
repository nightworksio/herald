//! What the instance last published to its peers: its key set and the
//! sequence it published it under (ADR-0044), each feed's envelopes and
//! the static signed log it last wrote (ADR-0060);
//! the key set each peer last handed over to (ADR-0049); and what each
//! peer asked of it, by day, and in its rate's window (ADR-0054).

use std::future::Future;

use herald_core::{AuditDay, DestinationName, Feed, PeerKeys, Publication, PublishedKeys, Window};

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

    /// The static signed log last written, where one was (ADR-0060).
    fn publication(
        &self,
    ) -> impl Future<Output = Result<Option<Versioned<Publication>>, StoreError>> + Send;

    /// Writes `publication` if the stored one is at `expected`, or absent
    /// when `expected` is `None`.
    fn put_publication(
        &self,
        publication: &Publication,
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

    /// What `peer` asked on `day`, where it asked anything.
    fn audit(
        &self,
        peer: &str,
        day: &str,
    ) -> impl Future<Output = Result<Option<Versioned<AuditDay>>, StoreError>> + Send;

    /// Writes `audit` under its peer and day if the stored one is at
    /// `expected`, or absent when `expected` is `None`.
    fn put_audit(
        &self,
        audit: &AuditDay,
        expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send;

    /// Every day kept of what `peer` asked, in no order.
    fn audits(&self, peer: &str) -> impl Future<Output = Result<Vec<AuditDay>, StoreError>> + Send;

    /// Forgets every day of every peer before `day`.
    fn expire_audits(&self, day: &str) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// The window of `peer`'s rate, where it asked anything.
    fn window(
        &self,
        peer: &str,
    ) -> impl Future<Output = Result<Option<Versioned<Window>>, StoreError>> + Send;

    /// Writes the window of `peer`'s rate as `window` if the stored one is
    /// at `expected`, or absent when `expected` is `None`.
    fn put_window(
        &self,
        peer: &str,
        window: &Window,
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

    fn publication(
        &self,
    ) -> impl Future<Output = Result<Option<Versioned<Publication>>, StoreError>> + Send {
        std::future::ready(Err(super::Held::Published.unsupported()))
    }

    fn put_publication(
        &self,
        _: &Publication,
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

    fn audit(
        &self,
        _: &str,
        _: &str,
    ) -> impl Future<Output = Result<Option<Versioned<AuditDay>>, StoreError>> + Send {
        std::future::ready(Err(super::Held::Published.unsupported()))
    }

    fn put_audit(
        &self,
        _: &AuditDay,
        _: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send {
        std::future::ready(Err(super::Held::Published.unsupported()))
    }

    fn audits(&self, _: &str) -> impl Future<Output = Result<Vec<AuditDay>, StoreError>> + Send {
        std::future::ready(Err(super::Held::Published.unsupported()))
    }

    fn expire_audits(&self, _: &str) -> impl Future<Output = Result<(), StoreError>> + Send {
        std::future::ready(Err(super::Held::Published.unsupported()))
    }

    fn window(
        &self,
        _: &str,
    ) -> impl Future<Output = Result<Option<Versioned<Window>>, StoreError>> + Send {
        std::future::ready(Err(super::Held::Published.unsupported()))
    }

    fn put_window(
        &self,
        _: &str,
        _: &Window,
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

    async fn publication(&self) -> Result<Option<Versioned<Publication>>, StoreError> {
        match self {
            Some(kept) => kept.publication().await,
            None => super::Unkept.publication().await,
        }
    }

    async fn put_publication(
        &self,
        publication: &Publication,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_publication(publication, expected).await,
            None => super::Unkept.put_publication(publication, expected).await,
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

    async fn audit(
        &self,
        peer: &str,
        day: &str,
    ) -> Result<Option<Versioned<AuditDay>>, StoreError> {
        match self {
            Some(kept) => kept.audit(peer, day).await,
            None => super::Unkept.audit(peer, day).await,
        }
    }

    async fn put_audit(
        &self,
        audit: &AuditDay,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_audit(audit, expected).await,
            None => super::Unkept.put_audit(audit, expected).await,
        }
    }

    async fn audits(&self, peer: &str) -> Result<Vec<AuditDay>, StoreError> {
        match self {
            Some(kept) => kept.audits(peer).await,
            None => super::Unkept.audits(peer).await,
        }
    }

    async fn expire_audits(&self, day: &str) -> Result<(), StoreError> {
        match self {
            Some(kept) => kept.expire_audits(day).await,
            None => super::Unkept.expire_audits(day).await,
        }
    }

    async fn window(&self, peer: &str) -> Result<Option<Versioned<Window>>, StoreError> {
        match self {
            Some(kept) => kept.window(peer).await,
            None => super::Unkept.window(peer).await,
        }
    }

    async fn put_window(
        &self,
        peer: &str,
        window: &Window,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self {
            Some(kept) => kept.put_window(peer, window, expected).await,
            None => super::Unkept.put_window(peer, window, expected).await,
        }
    }
}
