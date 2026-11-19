use herald_core::{KeptPull, PullKey, Reconciled, RepositoryKey, SourceName};
use herald_ports::{Pulls, StoreError, Version, Versioned, Written};
use jiff::Timestamp;

use super::{Failing, Fault, Refusing, broken};

impl Pulls for Failing {
    async fn pull(&self, key: &PullKey) -> Result<Option<Versioned<KeptPull>>, StoreError> {
        if self.fault == Fault::ReadsPulls {
            return Err(broken());
        }
        self.inner.pull(key).await
    }

    async fn pulls_of(
        &self,
        repository: &RepositoryKey,
    ) -> Result<Vec<Versioned<KeptPull>>, StoreError> {
        if self.fault == Fault::ListsPulls {
            return Err(broken());
        }
        self.inner.pulls_of(repository).await
    }

    async fn put_pull(
        &self,
        kept: &KeptPull,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self.fault {
            Fault::WritesPulls => Err(broken()),
            Fault::RacesPulls => Ok(Written::Conflict),
            _ => self.inner.put_pull(kept, expected).await,
        }
    }

    async fn expire_pulls(&self, before: Timestamp) -> Result<(), StoreError> {
        if self.fault == Fault::ExpiresPulls {
            return Err(broken());
        }
        self.inner.expire_pulls(before).await
    }

    async fn reconciled(
        &self,
        source: &SourceName,
    ) -> Result<Option<Versioned<Reconciled>>, StoreError> {
        if self.fault == Fault::ReadsReconciles {
            return Err(broken());
        }
        self.inner.reconciled(source).await
    }

    async fn put_reconciled(
        &self,
        source: &SourceName,
        reconciled: &Reconciled,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        if self.fault == Fault::WritesReconciles {
            return Err(broken());
        }
        self.inner
            .put_reconciled(source, reconciled, expected)
            .await
    }
}

impl Pulls for Refusing {
    async fn pull(&self, _: &PullKey) -> Result<Option<Versioned<KeptPull>>, StoreError> {
        Err(self.0.clone())
    }

    async fn pulls_of(&self, _: &RepositoryKey) -> Result<Vec<Versioned<KeptPull>>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_pull(&self, _: &KeptPull, _: Option<Version>) -> Result<Written, StoreError> {
        Err(self.0.clone())
    }

    async fn expire_pulls(&self, _: Timestamp) -> Result<(), StoreError> {
        Err(self.0.clone())
    }

    async fn reconciled(
        &self,
        _: &SourceName,
    ) -> Result<Option<Versioned<Reconciled>>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_reconciled(
        &self,
        _: &SourceName,
        _: &Reconciled,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        Err(self.0.clone())
    }
}
