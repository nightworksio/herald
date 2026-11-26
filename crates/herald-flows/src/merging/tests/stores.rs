//! Stores a queue's judging is tested against: one whose records cannot be
//! read or written, and one a dry run must never write to.

use herald_core::merging::{GestureKey, GestureRecord};
use herald_ports::{Actions, Version, Versioned, Written};

#[derive(Debug)]
pub(super) struct Unkept(pub(super) bool);

impl Actions for Unkept {
    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn action(
        &self,
        _: &herald_core::RunKey,
    ) -> Result<Option<Versioned<herald_core::RerunRecord>>, herald_ports::StoreError> {
        unreachable!("a queue reads no re-run")
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn put_action(
        &self,
        _: &herald_core::RerunRecord,
        _: Option<Version>,
    ) -> Result<Written, herald_ports::StoreError> {
        unreachable!("a queue writes no re-run")
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn gesture(
        &self,
        _: &GestureKey,
    ) -> Result<Option<Versioned<GestureRecord>>, herald_ports::StoreError> {
        if self.0 {
            Ok(None)
        } else {
            Err(herald_ports::StoreError::Unavailable {
                reason: "down".to_owned(),
            })
        }
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn put_gesture(
        &self,
        _: &GestureRecord,
        _: Option<Version>,
    ) -> Result<Written, herald_ports::StoreError> {
        Ok(Written::Conflict)
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn left_record(
        &self,
        _: &herald_core::merging::LeftKey,
    ) -> Result<Option<Versioned<herald_core::merging::LeftRecord>>, herald_ports::StoreError> {
        unreachable!("judging a queue reads no leaving")
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn put_left_record(
        &self,
        _: &herald_core::merging::LeftRecord,
        _: Option<Version>,
    ) -> Result<Written, herald_ports::StoreError> {
        unreachable!("judging a queue writes no leaving")
    }
}

#[derive(Debug)]
pub(super) struct ReadOnly;

impl Actions for ReadOnly {
    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn action(
        &self,
        _: &herald_core::RunKey,
    ) -> Result<Option<Versioned<herald_core::RerunRecord>>, herald_ports::StoreError> {
        unreachable!("a queue reads no re-run")
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn put_action(
        &self,
        _: &herald_core::RerunRecord,
        _: Option<Version>,
    ) -> Result<Written, herald_ports::StoreError> {
        unreachable!("a queue writes no re-run")
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn gesture(
        &self,
        _: &GestureKey,
    ) -> Result<Option<Versioned<GestureRecord>>, herald_ports::StoreError> {
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
    ) -> Result<Written, herald_ports::StoreError> {
        panic!("a dry run writes no record")
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn left_record(
        &self,
        _: &herald_core::merging::LeftKey,
    ) -> Result<Option<Versioned<herald_core::merging::LeftRecord>>, herald_ports::StoreError> {
        unreachable!("judging a queue reads no leaving")
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn put_left_record(
        &self,
        _: &herald_core::merging::LeftRecord,
        _: Option<Version>,
    ) -> Result<Written, herald_ports::StoreError> {
        unreachable!("judging a queue writes no leaving")
    }
}
