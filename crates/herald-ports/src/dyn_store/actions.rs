//! The record of each re-run, of each gesture and of each pull request's
//! last leaving of herald's own queue, read and written through an erased
//! store (ADR-0068, ADR-0087).

use herald_core::merging::{GestureKey, GestureRecord, LeftKey, LeftRecord};
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

    /// [`Actions::gesture`].
    fn gesture_boxed<'a>(
        &'a self,
        key: &'a GestureKey,
    ) -> Boxed<'a, Result<Option<Versioned<GestureRecord>>, StoreError>>;

    /// [`Actions::put_gesture`].
    fn put_gesture_boxed<'a>(
        &'a self,
        gesture: &'a GestureRecord,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>>;

    /// [`Actions::left_record`].
    fn left_record_boxed<'a>(
        &'a self,
        key: &'a LeftKey,
    ) -> Boxed<'a, Result<Option<Versioned<LeftRecord>>, StoreError>>;

    /// [`Actions::put_left_record`].
    fn put_left_record_boxed<'a>(
        &'a self,
        left: &'a LeftRecord,
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

    fn gesture_boxed<'a>(
        &'a self,
        key: &'a GestureKey,
    ) -> Boxed<'a, Result<Option<Versioned<GestureRecord>>, StoreError>> {
        Box::pin(async move { self.actions().gesture(key).await })
    }

    fn put_gesture_boxed<'a>(
        &'a self,
        gesture: &'a GestureRecord,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.actions().put_gesture(gesture, expected).await })
    }

    fn left_record_boxed<'a>(
        &'a self,
        key: &'a LeftKey,
    ) -> Boxed<'a, Result<Option<Versioned<LeftRecord>>, StoreError>> {
        Box::pin(async move { self.actions().left_record(key).await })
    }

    fn put_left_record_boxed<'a>(
        &'a self,
        left: &'a LeftRecord,
        expected: Option<Version>,
    ) -> Boxed<'a, Result<Written, StoreError>> {
        Box::pin(async move { self.actions().put_left_record(left, expected).await })
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

    async fn gesture(
        &self,
        key: &GestureKey,
    ) -> Result<Option<Versioned<GestureRecord>>, StoreError> {
        self.0.gesture_boxed(key).await
    }

    async fn put_gesture(
        &self,
        gesture: &GestureRecord,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0.put_gesture_boxed(gesture, expected).await
    }

    async fn left_record(
        &self,
        key: &LeftKey,
    ) -> Result<Option<Versioned<LeftRecord>>, StoreError> {
        self.0.left_record_boxed(key).await
    }

    async fn put_left_record(
        &self,
        left: &LeftRecord,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.0.put_left_record_boxed(left, expected).await
    }
}
