//! The reports whose living posts were sent advisories, read through an
//! erased store (ADR-0067).

use herald_core::AdvisedReports;

use super::DynStore;
use crate::erased::{Boxed, Erased};
use crate::store::{Recalls, Store, StoreError, Version, Versioned, Written};

/// [`Recalls`], boxed so a store object answers it.
pub trait DynRecalls: Sync {
    /// [`Recalls::advised`].
    fn advised_boxed(&self) -> Boxed<'_, Result<Option<Versioned<AdvisedReports>>, StoreError>>;

    /// [`Recalls::put_advised`].
    fn put_advised_boxed<'a>(
        &'a self,
        reports: &'a AdvisedReports,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;
}

impl<S: Store> DynRecalls for S {
    fn advised_boxed(&self) -> Boxed<'_, Result<Option<Versioned<AdvisedReports>>, StoreError>> {
        Box::pin(async move { self.recalls().advised().await })
    }

    fn put_advised_boxed<'a>(
        &'a self,
        reports: &'a AdvisedReports,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.recalls().put_advised(reports, expected).await })
    }
}

impl Recalls for Erased<'_, dyn DynStore + '_> {
    async fn advised(&self) -> Result<Option<Versioned<AdvisedReports>>, StoreError> {
        self.0.advised_boxed().await
    }

    async fn put_advised(
        &self,
        reports: &AdvisedReports,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0.put_advised_boxed(reports, expected).await
    }
}
