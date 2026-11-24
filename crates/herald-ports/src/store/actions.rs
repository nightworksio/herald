//! The record of each re-run an operator applies, and of each gesture that
//! queued a pull request, written with compare-and-set before the call
//! (ADR-0068, ADR-0087).

use std::future::{Future, ready};

use herald_core::merging::{GestureKey, GestureRecord};
use herald_core::{RerunRecord, RunKey};

use super::{Held, StoreError, Unkept, Version, Versioned, Written};

/// One record per run and head herald was asked to re-run, written as new
/// before the call, so two applies racing or a crash after the call never
/// lead to a second re-run (ADR-0068).
pub trait Actions: Sync {
    /// The record of the run `key`, where one is kept.
    fn action(
        &self,
        key: &RunKey,
    ) -> impl Future<Output = Result<Option<Versioned<RerunRecord>>, StoreError>> + Send;

    /// Writes `action` if its stored record is at `expected`, or absent
    /// when `expected` is `None`.
    fn put_action(
        &self,
        action: &RerunRecord,
        expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send;

    /// The record of the gesture `key` that queued a pull request, where
    /// one is kept (ADR-0087).
    fn gesture(
        &self,
        key: &GestureKey,
    ) -> impl Future<Output = Result<Option<Versioned<GestureRecord>>, StoreError>> + Send;

    /// Writes `gesture` if its stored record is at `expected`, or absent
    /// when `expected` is `None`.
    fn put_gesture(
        &self,
        gesture: &GestureRecord,
        expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send;
}

impl Actions for Unkept {
    fn action(
        &self,
        _: &RunKey,
    ) -> impl Future<Output = Result<Option<Versioned<RerunRecord>>, StoreError>> + Send {
        ready(Err(Held::Actions.unsupported()))
    }

    fn put_action(
        &self,
        _: &RerunRecord,
        _: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send {
        ready(Err(Held::Actions.unsupported()))
    }

    fn gesture(
        &self,
        _: &GestureKey,
    ) -> impl Future<Output = Result<Option<Versioned<GestureRecord>>, StoreError>> + Send {
        ready(Err(Held::Actions.unsupported()))
    }

    fn put_gesture(
        &self,
        _: &GestureRecord,
        _: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send {
        ready(Err(Held::Actions.unsupported()))
    }
}
