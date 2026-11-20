use herald_core::{KeptPull, PullKey, Reconciled, RepositoryKey, SourceName};
use herald_ports::{Pulls, StoreError, Version, Versioned, Written};
use jiff::Timestamp;

use super::{Fault, Faulty, refused};
use crate::conformance::pulls::{kept, repository};

impl Pulls for Faulty {
    async fn pull(&self, key: &PullKey) -> Result<Option<Versioned<KeptPull>>, StoreError> {
        self.refuses()?;
        let read = self.inner.pull(key).await?;
        if self.fault == Fault::HoldsAnUnwrittenPull && read.is_none() {
            return Ok(Some(Versioned {
                value: kept(&key.repository.id, key.number, "held"),
                version: Version(1),
            }));
        }
        Ok(read)
    }

    async fn pulls_of(
        &self,
        repository_key: &RepositoryKey,
    ) -> Result<Vec<Versioned<KeptPull>>, StoreError> {
        self.refuses()?;
        let mut listed = self.inner.pulls_of(repository_key).await?;
        if self.fault == Fault::ListsOtherRepositoriesPulls {
            listed.extend(self.inner.pulls_of(&repository("4")).await?);
        }
        Ok(listed)
    }

    async fn put_pull(
        &self,
        pull: &KeptPull,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.refuses()?;
        let current = self.inner.pull(&pull.key).await?.map(|read| read.version);
        match self.fault {
            Fault::OverwritesAnyPullVersion => self.inner.put_pull(pull, current).await,
            Fault::OverwritesAStalePullVersion if expected.is_some() => {
                self.inner.put_pull(pull, current).await
            }
            Fault::KeepsTheFirstPull if expected.is_some() && expected == current => {
                Ok(Written::Written(Version(9)))
            }
            _ => self.inner.put_pull(pull, expected).await,
        }
    }

    async fn expire_pulls(&self, before: Timestamp) -> Result<(), StoreError> {
        self.refuses()?;
        match self.fault {
            Fault::ExpiresNoPull => Ok(()),
            Fault::ExpiresEveryClosedPull => self.inner.expire_pulls(Timestamp::MAX).await,
            _ => self.inner.expire_pulls(before).await,
        }
    }

    async fn reconciled(
        &self,
        source: &SourceName,
    ) -> Result<Option<Versioned<Reconciled>>, StoreError> {
        self.refuses()?;
        match self.fault {
            Fault::HoldsAnUnwrittenReconcile => Ok(Some(Versioned {
                value: Reconciled {
                    repositories: std::collections::BTreeSet::new(),
                    repository: None,
                    at: None,
                    validators: Vec::new(),
                },
                version: Version(1),
            })),
            Fault::SharesReconciles => self.inner.reconciled(&SourceName::new("herald")).await,
            _ => self.inner.reconciled(source).await,
        }
    }

    async fn put_reconciled(
        &self,
        source: &SourceName,
        reconciled: &Reconciled,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.refuses()?;
        let current = self
            .inner
            .reconciled(source)
            .await?
            .map(|read| read.version);
        match self.fault {
            Fault::OverwritesAnyReconcileVersion => {
                self.inner.put_reconciled(source, reconciled, current).await
            }
            Fault::KeepsTheFirstReconcile if expected.is_some() && expected == current => {
                Ok(Written::Written(Version(9)))
            }
            _ => {
                self.inner
                    .put_reconciled(source, reconciled, expected)
                    .await
            }
        }
    }
}

#[test]
fn a_store_that_breaks_its_promises_on_pull_requests_is_refused() {
    refused(&[
        (
            Fault::HoldsAnUnwrittenPull,
            "a new store holds a pull request",
        ),
        (
            Fault::OverwritesAnyPullVersion,
            "a pull request was written as new over one that exists",
        ),
        (
            Fault::OverwritesAStalePullVersion,
            "a pull request was written over a version it did not expect",
        ),
        (
            Fault::KeepsTheFirstPull,
            "a pull request was not the one written last under its key",
        ),
        (
            Fault::ListsOtherRepositoriesPulls,
            "a repository's pull requests were not its own and every one",
        ),
        (
            Fault::ExpiresNoPull,
            "a pull request closed before the expiry was kept",
        ),
        (
            Fault::ExpiresEveryClosedPull,
            "an open pull request, or one closed at the expiry, was forgotten",
        ),
        (
            Fault::HoldsAnUnwrittenReconcile,
            "a new store holds a reconcile",
        ),
        (
            Fault::OverwritesAnyReconcileVersion,
            "a reconcile was written over a version it did not expect",
        ),
        (
            Fault::KeepsTheFirstReconcile,
            "a reconcile was not the one written last for its source",
        ),
        (
            Fault::SharesReconciles,
            "a reconcile was not the one written last for its source",
        ),
    ]);
}
