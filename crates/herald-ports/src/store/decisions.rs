//! What models answered the tasks views asked (ADR-0057).

use std::future::{Future, ready};

use herald_core::{Decided, Key};
use jiff::Timestamp;

use super::{Held, StoreError, Unkept};

/// What models answered the tasks views asked, one record per task,
/// subject and input, reused while the input stands and forgotten by
/// housekeeping (ADR-0057).
pub trait Decisions: Sync {
    /// The decision keyed `key`, where one is kept.
    fn decision(
        &self,
        key: &Key,
    ) -> impl Future<Output = Result<Option<Decided>, StoreError>> + Send;

    /// Keeps `decided` under its key, in place of any kept there.
    fn put_decision(
        &self,
        decided: &Decided,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Forgets every decision made before `before`.
    fn expire_decisions(
        &self,
        before: Timestamp,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

impl Decisions for Unkept {
    fn decision(
        &self,
        _: &Key,
    ) -> impl Future<Output = Result<Option<Decided>, StoreError>> + Send {
        ready(Err(Held::Decisions.unsupported()))
    }

    fn put_decision(&self, _: &Decided) -> impl Future<Output = Result<(), StoreError>> + Send {
        ready(Err(Held::Decisions.unsupported()))
    }

    fn expire_decisions(
        &self,
        _: Timestamp,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        ready(Err(Held::Decisions.unsupported()))
    }
}
