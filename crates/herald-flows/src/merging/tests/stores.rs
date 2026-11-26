//! The store a queue's judging is tested against: one whose records cannot
//! be read, one whose writes all conflict, or one a dry run must never
//! write to.

use herald_core::merging::{GestureKey, GestureRecord, LeftKey, LeftRecord};
use herald_core::{RerunRecord, RunKey};
use herald_ports::{Actions, StoreError, Version, Versioned, Written};

/// How a judged queue's store answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Judging {
    /// Every read of a gesture fails, and every write conflicts.
    Down,
    /// No gesture is kept, and every write conflicts.
    Conflicting,
    /// No gesture is kept, and a write is a test's failure.
    ReadOnly,
}

impl Actions for Judging {
    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn action(&self, _: &RunKey) -> Result<Option<Versioned<RerunRecord>>, StoreError> {
        unreachable!("a queue reads no re-run")
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn put_action(&self, _: &RerunRecord, _: Option<Version>) -> Result<Written, StoreError> {
        unreachable!("a queue writes no re-run")
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn gesture(
        &self,
        _: &GestureKey,
    ) -> Result<Option<Versioned<GestureRecord>>, StoreError> {
        if *self == Self::Down {
            return Err(StoreError::Unavailable {
                reason: "down".to_owned(),
            });
        }
        Ok(None)
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn put_gesture(
        &self,
        _: &GestureRecord,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        assert_ne!(*self, Self::ReadOnly, "a dry run writes no record");
        Ok(Written::Conflict)
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn left_record(&self, _: &LeftKey) -> Result<Option<Versioned<LeftRecord>>, StoreError> {
        unreachable!("judging a queue reads no leaving")
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn put_left_record(
        &self,
        _: &LeftRecord,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        unreachable!("judging a queue writes no leaving")
    }
}
