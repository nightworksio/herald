use herald_core::AdvisedReports;
use herald_ports::{Recalls, StoreError, Version, Versioned, Written};

use super::{Failing, Fault, Refusing, broken};

impl Recalls for Failing {
    async fn advised(&self) -> Result<Option<Versioned<AdvisedReports>>, StoreError> {
        if self.fault == Fault::ReadsRecalls {
            return Err(broken());
        }
        self.inner.advised().await
    }

    async fn put_advised(
        &self,
        reports: &AdvisedReports,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        match self.fault {
            Fault::WritesRecalls => Err(broken()),
            Fault::RecallsConflict => Ok(Written::Conflict),
            _ => self.inner.put_advised(reports, expected).await,
        }
    }
}

impl Recalls for Refusing {
    async fn advised(&self) -> Result<Option<Versioned<AdvisedReports>>, StoreError> {
        Err(self.0.clone())
    }

    async fn put_advised(
        &self,
        _: &AdvisedReports,
        _: Option<Version>,
    ) -> Result<Written, StoreError> {
        Err(self.0.clone())
    }
}
