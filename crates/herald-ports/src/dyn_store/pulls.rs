//! The pull requests kept mode keeps, read through an erased store.

use herald_core::{KeptPull, PullKey, Reconciled, RepositoryKey, SourceName};
use jiff::Timestamp;

use super::DynStore;
use crate::erased::Erased;
use crate::store::{Pulls, StoreError, Version, Versioned, Written};

impl Pulls for Erased<'_, dyn DynStore + '_> {
    async fn pull(&self, key: &PullKey) -> Result<Option<Versioned<KeptPull>>, StoreError> {
        self.0.pull_boxed(key).await
    }

    async fn pulls_of(
        &self,
        repository: &RepositoryKey,
    ) -> Result<Vec<Versioned<KeptPull>>, StoreError> {
        self.0.pulls_of_boxed(repository).await
    }

    async fn put_pull(
        &self,
        kept: &KeptPull,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0.put_pull_boxed(kept, expected).await
    }

    async fn expire_pulls(&self, before: Timestamp) -> Result<(), StoreError> {
        self.0.expire_pulls_of_boxed(before).await
    }

    async fn reconciled(
        &self,
        source: &SourceName,
    ) -> Result<Option<Versioned<Reconciled>>, StoreError> {
        self.0.reconciled_boxed(source).await
    }

    async fn put_reconciled(
        &self,
        source: &SourceName,
        reconciled: &Reconciled,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0
            .put_reconciled_boxed(source, reconciled, expected)
            .await
    }
}
