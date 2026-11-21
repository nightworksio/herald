//! The names of the reports whose living posts were sent advisories, one
//! record (ADR-0067).

use herald_core::AdvisedReports;
use herald_ports::{Recalls, StoreError, Version, Versioned, Written};

use super::RecordStore;
use crate::paths;
use crate::records::RecordBackend;

impl<R: RecordBackend> Recalls for RecordStore<R> {
    async fn advised(&self) -> Result<Option<Versioned<AdvisedReports>>, StoreError> {
        self.record(paths::RECALLS).await
    }

    async fn put_advised(
        &self,
        reports: &AdvisedReports,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.compare_and_set(paths::RECALLS, reports, expected)
            .await
    }
}
