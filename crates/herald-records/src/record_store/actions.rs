//! The record of each re-run an operator applied, one record per run and
//! head in one directory (ADR-0068), and of each gesture that queued a pull
//! request, one record per gesture in another (ADR-0087).

use herald_core::merging::{GestureKey, GestureRecord};
use herald_core::{RerunRecord, RunKey};
use herald_ports::{Actions, StoreError, Version, Versioned, Written};

use super::RecordStore;
use crate::paths;
use crate::records::RecordBackend;

impl<R: RecordBackend> Actions for RecordStore<R> {
    async fn action(&self, key: &RunKey) -> Result<Option<Versioned<RerunRecord>>, StoreError> {
        self.record(&paths::action(key)).await
    }

    async fn put_action(
        &self,
        action: &RerunRecord,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.compare_and_set(&paths::action(&action.key), action, expected)
            .await
    }

    async fn gesture(
        &self,
        key: &GestureKey,
    ) -> Result<Option<Versioned<GestureRecord>>, StoreError> {
        self.record(&paths::gesture(key)).await
    }

    async fn put_gesture(
        &self,
        gesture: &GestureRecord,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.compare_and_set(&paths::gesture(&gesture.key), gesture, expected)
            .await
    }
}
