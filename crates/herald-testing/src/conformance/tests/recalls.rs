use herald_core::AdvisedReports;
use herald_ports::{Recalls, StoreError, Version, Versioned, Written};

use super::{Fault, Faulty, refused};
use crate::conformance::recalls::named;

impl Recalls for Faulty {
    async fn advised(&self) -> Result<Option<Versioned<AdvisedReports>>, StoreError> {
        self.refuses()?;
        let read = self.inner.advised().await?;
        if self.fault == Fault::HoldsUnwrittenRecalls && read.is_none() {
            return Ok(Some(Versioned {
                value: named(&["held"]),
                version: Version(1),
            }));
        }
        Ok(read)
    }

    async fn put_advised(
        &self,
        reports: &AdvisedReports,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.refuses()?;
        let current = self.inner.advised().await?;
        match self.fault {
            Fault::OverwritesAnyRecallsVersion if expected.is_none() => {
                let version = current.map(|current| current.version);
                self.inner.put_advised(reports, version).await
            }
            Fault::OverwritesAStaleRecallsVersion if expected.is_some() => {
                let version = current.map(|current| current.version);
                self.inner.put_advised(reports, version).await
            }
            Fault::KeepsTheFirstRecalls if expected.is_some() => match current {
                Some(current) => self.inner.put_advised(&current.value, expected).await,
                None => self.inner.put_advised(reports, expected).await,
            },
            _ => self.inner.put_advised(reports, expected).await,
        }
    }
}

#[test]
fn a_store_that_breaks_its_promises_on_the_names_of_advised_reports_is_refused() {
    refused(&[
        (
            Fault::HoldsUnwrittenRecalls,
            "a new store holds names of reports whose living posts were sent advisories",
        ),
        (
            Fault::OverwritesAnyRecallsVersion,
            "the names of reports were written as new over a record that exists",
        ),
        (
            Fault::OverwritesAStaleRecallsVersion,
            "the names of reports were written over a stale version",
        ),
        (
            Fault::KeepsTheFirstRecalls,
            "the names of reports did not read back as last written",
        ),
    ]);
}
