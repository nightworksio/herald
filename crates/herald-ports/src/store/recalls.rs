//! The names of the reports whose cursor may keep a living post an
//! advisories section was sent in (ADR-0067).

use std::future::{Future, ready};

use herald_core::AdvisedReports;

use super::{Held, StoreError, Unkept, Version, Versioned, Written};

/// One record naming every report whose cursor may keep a living post an
/// advisories section was sent in, written with compare-and-set before the
/// cursor, so herald finds each such post again once its report is gone
/// (ADR-0067).
pub trait Recalls: Sync {
    /// The reports named, where the record is kept.
    fn advised(
        &self,
    ) -> impl Future<Output = Result<Option<Versioned<AdvisedReports>>, StoreError>> + Send;

    /// Writes `reports` if the stored record is at `expected`, or absent
    /// when `expected` is `None`.
    fn put_advised(
        &self,
        reports: &AdvisedReports,
        expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send;
}

impl Recalls for Unkept {
    fn advised(
        &self,
    ) -> impl Future<Output = Result<Option<Versioned<AdvisedReports>>, StoreError>> + Send {
        ready(Err(Held::Recalls.unsupported()))
    }

    fn put_advised(
        &self,
        _: &AdvisedReports,
        _: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send {
        ready(Err(Held::Recalls.unsupported()))
    }
}
