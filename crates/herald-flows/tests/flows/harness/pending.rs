use herald_core::{Hold, Pending};
use herald_ports::{StoreError, Version, Versioned, Waiting, Written};

use super::{Failing, Fault, Refusing, broken};

impl Waiting for Failing {
    async fn pending(&self, hold: &Hold) -> Result<Option<Versioned<Pending>>, StoreError> {
        let group = matches!(hold, Hold::Group { .. });
        if self.fault == Fault::ReadsHeld || (self.fault == Fault::ReadsGroups && group) {
            return Err(broken());
        }
        self.inner.pending(hold).await
    }

    async fn every_pending(&self) -> Result<Vec<Versioned<Pending>>, StoreError> {
        if self.fault == Fault::ReadsPending {
            return Err(broken());
        }
        self.inner.every_pending().await
    }

    async fn put_pending(
        &self,
        pending: &Pending,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        let closing = pending.closed.is_some() || pending.landed;
        let group = matches!(pending.hold, Hold::Group { .. });
        match self.fault {
            Fault::WritesPending => Err(broken()),
            Fault::WritesGroups if group => Err(broken()),
            Fault::PendingConflicts => Ok(Written::Conflict),
            Fault::ClosingConflicts if closing => Ok(Written::Conflict),
            _ => self.inner.put_pending(pending, expected).await,
        }
    }

    async fn drop_pending(&self, hold: &Hold) -> Result<(), StoreError> {
        if self.fault == Fault::DropsPending {
            return Err(broken());
        }
        self.inner.drop_pending(hold).await
    }
}

impl Waiting for Refusing {
    async fn pending(&self, _: &Hold) -> Result<Option<Versioned<Pending>>, StoreError> {
        Err(self.0.clone())
    }

    async fn every_pending(&self) -> Result<Vec<Versioned<Pending>>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_pending(&self, _: &Pending, _: Option<Version>) -> Result<Written, StoreError> {
        Err(self.0.clone())
    }

    async fn drop_pending(&self, _: &Hold) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Tokens for Failing {
    async fn token(
        &self,
        name: &str,
    ) -> Result<Option<Versioned<herald_core::KeptToken>>, StoreError> {
        if self.fault == Fault::ReadsTokens {
            return Err(broken());
        }
        self.inner.token(name).await
    }

    async fn put_token(
        &self,
        token: &herald_core::KeptToken,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        if self.fault == Fault::WritesTokens {
            return Err(broken());
        }
        self.inner.put_token(token, expected).await
    }
}

impl herald_ports::Tokens for Refusing {
    async fn token(
        &self,
        _: &str,
    ) -> Result<Option<Versioned<herald_core::KeptToken>>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_token(
        &self,
        _: &herald_core::KeptToken,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        Err(self.0.clone())
    }
}

impl herald_ports::Published for Failing {
    async fn published_keys(
        &self,
    ) -> Result<Option<Versioned<herald_core::PublishedKeys>>, StoreError> {
        if self.fault == Fault::ReadsPublished {
            return Err(broken());
        }
        self.inner.published_keys().await
    }

    async fn put_published_keys(
        &self,
        keys: &herald_core::PublishedKeys,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        if self.fault == Fault::WritesPublished {
            return Err(broken());
        }
        if self.fault == Fault::ConflictsPublished {
            return Ok(Written::Conflict);
        }
        self.inner.put_published_keys(keys, expected).await
    }

    async fn feed(
        &self,
        destination: &herald_core::DestinationName,
    ) -> Result<Option<Versioned<herald_core::Feed>>, StoreError> {
        if self.fault == Fault::ReadsFeeds {
            return Err(broken());
        }
        self.inner.feed(destination).await
    }

    async fn put_feed(
        &self,
        destination: &herald_core::DestinationName,
        feed: &herald_core::Feed,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        if self.fault == Fault::WritesFeeds {
            return Err(broken());
        }
        if self.fault == Fault::ConflictsFeeds {
            return Ok(Written::Conflict);
        }
        self.inner.put_feed(destination, feed, expected).await
    }
}

impl herald_ports::Published for Refusing {
    async fn published_keys(
        &self,
    ) -> Result<Option<Versioned<herald_core::PublishedKeys>>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_published_keys(
        &self,
        _: &herald_core::PublishedKeys,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        Err(self.0.clone())
    }

    async fn feed(
        &self,
        _: &herald_core::DestinationName,
    ) -> Result<Option<Versioned<herald_core::Feed>>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_feed(
        &self,
        _: &herald_core::DestinationName,
        _: &herald_core::Feed,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        Err(self.0.clone())
    }
}
