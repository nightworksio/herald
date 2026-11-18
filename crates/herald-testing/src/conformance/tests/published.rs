use super::*;

impl herald_ports::Published for Faulty {
    async fn published_keys(
        &self,
    ) -> Result<Option<herald_ports::Versioned<herald_core::PublishedKeys>>, StoreError> {
        let kept = self.inner.published_keys().await?;
        Ok(kept
            .filter(|kept| self.fault != Fault::ForgetsPublishedKeys || kept.value.sequence == 1))
    }

    async fn put_published_keys(
        &self,
        keys: &herald_core::PublishedKeys,
        expected: Option<herald_ports::Version>,
    ) -> Result<herald_ports::Written, StoreError> {
        let expected = match expected {
            Some(_) if self.fault == Fault::OverwritesAnyPublishedVersion => {
                self.inner.published_keys().await?.map(|kept| kept.version)
            }
            expected => expected,
        };
        self.inner.put_published_keys(keys, expected).await
    }

    async fn feed(
        &self,
        destination: &herald_core::DestinationName,
    ) -> Result<Option<herald_ports::Versioned<herald_core::Feed>>, StoreError> {
        let kept = match self.fault {
            Fault::SharesFeeds => {
                self.inner
                    .feed(&herald_core::DestinationName::new("releases"))
                    .await?
            }
            _ => self.inner.feed(destination).await?,
        };
        Ok(kept.filter(|kept| self.fault != Fault::ForgetsFeeds || kept.value.newest == 1))
    }

    async fn put_feed(
        &self,
        destination: &herald_core::DestinationName,
        feed: &herald_core::Feed,
        expected: Option<herald_ports::Version>,
    ) -> Result<herald_ports::Written, StoreError> {
        let expected = match expected {
            Some(_) if self.fault == Fault::OverwritesAnyFeedVersion => {
                self.inner.feed(destination).await?.map(|kept| kept.version)
            }
            expected => expected,
        };
        self.inner.put_feed(destination, feed, expected).await
    }

    async fn peer_keys(
        &self,
        peer: &str,
    ) -> Result<Option<herald_ports::Versioned<herald_core::PeerKeys>>, StoreError> {
        let kept = match self.fault {
            Fault::SharesPeerKeys => self.inner.peer_keys("upstream").await?,
            _ => self.inner.peer_keys(peer).await?,
        };
        Ok(kept.filter(|kept| self.fault != Fault::ForgetsPeerKeys || kept.value.sequence == 1))
    }

    async fn put_peer_keys(
        &self,
        peer: &str,
        keys: &herald_core::PeerKeys,
        expected: Option<herald_ports::Version>,
    ) -> Result<herald_ports::Written, StoreError> {
        let expected = match expected {
            Some(_) if self.fault == Fault::OverwritesAnyPeerKeysVersion => {
                self.inner.peer_keys(peer).await?.map(|kept| kept.version)
            }
            expected => expected,
        };
        self.inner.put_peer_keys(peer, keys, expected).await
    }

    async fn audit(
        &self,
        peer: &str,
        day: &str,
    ) -> Result<Option<herald_ports::Versioned<herald_core::AuditDay>>, StoreError> {
        match self.fault {
            Fault::SharesAudits => self.inner.audit("upstream", "2027-01-02").await,
            _ => self.inner.audit(peer, day).await,
        }
    }

    async fn put_audit(
        &self,
        audit: &herald_core::AuditDay,
        expected: Option<herald_ports::Version>,
    ) -> Result<herald_ports::Written, StoreError> {
        let expected = match expected {
            Some(_) if self.fault == Fault::OverwritesAnyAuditVersion => self
                .inner
                .audit(&audit.peer, &audit.day)
                .await?
                .map(|kept| kept.version),
            expected => expected,
        };
        self.inner.put_audit(audit, expected).await
    }

    async fn audits(&self, peer: &str) -> Result<Vec<herald_core::AuditDay>, StoreError> {
        match self.fault {
            Fault::SharesAudits => self.inner.audits("upstream").await,
            _ => self.inner.audits(peer).await,
        }
    }

    async fn expire_audits(&self, day: &str) -> Result<(), StoreError> {
        match self.fault {
            Fault::KeepsExpiredAudits => Ok(()),
            _ => self.inner.expire_audits(day).await,
        }
    }

    async fn window(
        &self,
        peer: &str,
    ) -> Result<Option<herald_ports::Versioned<herald_core::Window>>, StoreError> {
        match self.fault {
            Fault::SharesWindows => self.inner.window("upstream").await,
            _ => self.inner.window(peer).await,
        }
    }

    async fn put_window(
        &self,
        peer: &str,
        window: &herald_core::Window,
        expected: Option<herald_ports::Version>,
    ) -> Result<herald_ports::Written, StoreError> {
        self.inner.put_window(peer, window, expected).await
    }
}

#[test]
fn a_store_that_forgets_its_published_key_set_or_writes_over_any_version_is_refused() {
    refused(&[
        (
            Fault::ForgetsPublishedKeys,
            "a published key set read back was not the one written last",
        ),
        (
            Fault::OverwritesAnyPublishedVersion,
            "a published key set was written over a version it did not expect",
        ),
    ]);
}

#[test]
fn a_store_that_forgets_a_feed_shares_feeds_or_writes_over_any_version_is_refused() {
    refused(&[
        (
            Fault::ForgetsFeeds,
            "a feed read back was not the one written last",
        ),
        (Fault::SharesFeeds, "a feed was kept as another feed's"),
        (
            Fault::OverwritesAnyFeedVersion,
            "a feed was written over a version it did not expect",
        ),
    ]);
}

#[test]
fn a_store_that_forgets_a_peer_s_key_set_shares_them_or_writes_over_any_version_is_refused() {
    refused(&[
        (
            Fault::ForgetsPeerKeys,
            "a peer's key set read back was not the one written last",
        ),
        (
            Fault::SharesPeerKeys,
            "a peer's key set was kept as another peer's",
        ),
        (
            Fault::OverwritesAnyPeerKeysVersion,
            "a peer's key set was written over a version it did not expect",
        ),
    ]);
}

#[test]
fn a_store_that_shares_or_keeps_peers_audits_or_windows_is_refused() {
    refused(&[
        (
            Fault::SharesAudits,
            "a peer's audit read back was not the one written last for its day",
        ),
        (
            Fault::KeepsExpiredAudits,
            "an audit before the day expired was kept, or one from it forgotten",
        ),
        (
            Fault::OverwritesAnyAuditVersion,
            "a peer's audit was written over a version it did not expect",
        ),
        (
            Fault::SharesWindows,
            "a peer's window read back was not its own written last",
        ),
    ]);
}
