use herald_core::{Hold, Pending};
use herald_ports::{StoreError, Version, Versioned, Waiting, Written};

use super::{Failing, Fault, Refusing, broken};

impl Waiting for Failing {
    async fn pending(&self, hold: &Hold) -> Result<Option<Versioned<Pending>>, StoreError> {
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
        if self.fault == Fault::WritesPending {
            return Err(broken());
        }
        self.inner.put_pending(pending, expected).await
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
