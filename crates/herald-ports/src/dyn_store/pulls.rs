//! The pull requests kept mode keeps, read through an erased store.

use herald_core::{
    BaseKey, CodeownersKey, KeptBase, KeptCodeowners, KeptPull, PullKey, Reconciled, RepositoryKey,
    SourceName,
};
use jiff::Timestamp;

use super::DynStore;
use crate::erased::{Boxed, Erased};
use crate::store::{Pulls, Store, StoreError, Version, Versioned, Written};

/// [`Pulls`], boxed so a store object answers it.
pub trait DynPulls: Sync {
    /// [`Pulls::pull`].
    fn pull_boxed<'a>(
        &'a self,
        key: &'a PullKey,
    ) -> Boxed<'a, Result<Option<Versioned<KeptPull>>, StoreError>>;

    /// [`Pulls::pulls_of`].
    fn pulls_of_boxed<'a>(
        &'a self,
        repository: &'a RepositoryKey,
    ) -> Boxed<'a, Result<Vec<Versioned<KeptPull>>, StoreError>>;

    /// [`Pulls::put_pull`].
    fn put_pull_boxed<'a>(
        &'a self,
        kept: &'a KeptPull,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// [`Pulls::expire_pulls`].
    fn expire_pulls_of_boxed(&self, before: Timestamp) -> Boxed<'_, Result<(), StoreError>>;

    /// [`Pulls::reconciled`].
    fn reconciled_boxed<'a>(
        &'a self,
        source: &'a SourceName,
    ) -> Boxed<'a, Result<Option<Versioned<Reconciled>>, StoreError>>;

    /// [`Pulls::put_reconciled`].
    fn put_reconciled_boxed<'a>(
        &'a self,
        source: &'a SourceName,
        reconciled: &'a Reconciled,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// [`Pulls::codeowners`].
    fn codeowners_boxed<'a>(
        &'a self,
        key: &'a CodeownersKey,
    ) -> Boxed<'a, Result<Option<Versioned<KeptCodeowners>>, StoreError>>;

    /// [`Pulls::put_codeowners`].
    fn put_codeowners_boxed<'a>(
        &'a self,
        kept: &'a KeptCodeowners,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// [`Pulls::base`].
    fn base_boxed<'a>(
        &'a self,
        key: &'a BaseKey,
    ) -> Boxed<'a, Result<Option<Versioned<KeptBase>>, StoreError>>;

    /// [`Pulls::put_base`].
    fn put_base_boxed<'a>(
        &'a self,
        kept: &'a KeptBase,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;
}

impl<S: Store> DynPulls for S {
    fn pull_boxed<'a>(
        &'a self,
        key: &'a PullKey,
    ) -> Boxed<'a, Result<Option<Versioned<KeptPull>>, StoreError>> {
        Box::pin(async move { self.pulls().pull(key).await })
    }

    fn pulls_of_boxed<'a>(
        &'a self,
        repository: &'a RepositoryKey,
    ) -> Boxed<'a, Result<Vec<Versioned<KeptPull>>, StoreError>> {
        Box::pin(async move { self.pulls().pulls_of(repository).await })
    }

    fn put_pull_boxed<'a>(
        &'a self,
        kept: &'a KeptPull,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.pulls().put_pull(kept, expected).await })
    }

    fn expire_pulls_of_boxed(&self, before: Timestamp) -> Boxed<'_, Result<(), StoreError>> {
        Box::pin(async move { self.pulls().expire_pulls(before).await })
    }

    fn reconciled_boxed<'a>(
        &'a self,
        source: &'a SourceName,
    ) -> Boxed<'a, Result<Option<Versioned<Reconciled>>, StoreError>> {
        Box::pin(async move { self.pulls().reconciled(source).await })
    }

    fn put_reconciled_boxed<'a>(
        &'a self,
        source: &'a SourceName,
        reconciled: &'a Reconciled,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move {
            self.pulls()
                .put_reconciled(source, reconciled, expected)
                .await
        })
    }

    fn codeowners_boxed<'a>(
        &'a self,
        key: &'a CodeownersKey,
    ) -> Boxed<'a, Result<Option<Versioned<KeptCodeowners>>, StoreError>> {
        Box::pin(async move { self.pulls().codeowners(key).await })
    }

    fn put_codeowners_boxed<'a>(
        &'a self,
        kept: &'a KeptCodeowners,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.pulls().put_codeowners(kept, expected).await })
    }

    fn base_boxed<'a>(
        &'a self,
        key: &'a BaseKey,
    ) -> Boxed<'a, Result<Option<Versioned<KeptBase>>, StoreError>> {
        Box::pin(async move { self.pulls().base(key).await })
    }

    fn put_base_boxed<'a>(
        &'a self,
        kept: &'a KeptBase,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.pulls().put_base(kept, expected).await })
    }
}

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

    async fn codeowners(
        &self,
        key: &CodeownersKey,
    ) -> Result<Option<Versioned<KeptCodeowners>>, StoreError> {
        self.0.codeowners_boxed(key).await
    }

    async fn put_codeowners(
        &self,
        kept: &KeptCodeowners,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0.put_codeowners_boxed(kept, expected).await
    }

    async fn base(&self, key: &BaseKey) -> Result<Option<Versioned<KeptBase>>, StoreError> {
        self.0.base_boxed(key).await
    }

    async fn put_base(
        &self,
        kept: &KeptBase,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0.put_base_boxed(kept, expected).await
    }
}
