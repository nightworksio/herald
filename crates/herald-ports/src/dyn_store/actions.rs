//! The record of each re-run, read and written through an erased store
//! (ADR-0068).

use herald_core::{RerunRecord, RunKey};

use super::DynStore;
use crate::erased::{Boxed, Erased};
use crate::store::{Actions, Store, StoreError, Version, Versioned, Written};

/// [`Actions`], boxed so a store object answers it.
pub trait DynActions: Sync {
    /// [`Actions::action`].
    fn action_boxed<'a>(
        &'a self,
        key: &'a RunKey,
    ) -> Boxed<'a, Result<Option<Versioned<RerunRecord>>, StoreError>>;

    /// [`Actions::put_action`].
    fn put_action_boxed<'a>(
        &'a self,
        action: &'a RerunRecord,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;
}

impl<S: Store> DynActions for S {
    fn action_boxed<'a>(
        &'a self,
        key: &'a RunKey,
    ) -> Boxed<'a, Result<Option<Versioned<RerunRecord>>, StoreError>> {
        Box::pin(async move { self.actions().action(key).await })
    }

    fn put_action_boxed<'a>(
        &'a self,
        action: &'a RerunRecord,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.actions().put_action(action, expected).await })
    }
}

impl Actions for Erased<'_, dyn DynStore + '_> {
    async fn action(&self, key: &RunKey) -> Result<Option<Versioned<RerunRecord>>, StoreError> {
        self.0.action_boxed(key).await
    }

    async fn put_action(
        &self,
        action: &RerunRecord,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0.put_action_boxed(action, expected).await
    }
}
