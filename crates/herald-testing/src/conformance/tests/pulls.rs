use herald_core::{
    BaseKey, CodeownersKey, KeptBase, KeptCodeowners, KeptPull, PullKey, Reconciled, RepositoryKey,
    SourceName,
};
use herald_ports::{Pulls, StoreError, Version, Versioned, Written};
use jiff::Timestamp;

use super::{Fault, Faulty, refused};
use crate::conformance::NOON;
use crate::conformance::pulls::{LONG_BRANCH, based, codeowned, kept, repository};

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
        let codeowned = before > NOON + jiff::SignedDuration::from_hours(1);
        let bases = before > crate::conformance::CODEOWNED.1;
        match self.fault {
            Fault::ExpiresNoBases if bases => Ok(()),
            Fault::ExpiresEveryBase if bases => self.inner.expire_pulls(Timestamp::MAX).await,
            Fault::ExpiresNoPull => Ok(()),
            Fault::ExpiresNoCodeowners if codeowned => Ok(()),
            Fault::ExpiresEveryClosedPull => self.inner.expire_pulls(Timestamp::MAX).await,
            Fault::ExpiresEveryCodeowners if codeowned => {
                self.inner.expire_pulls(Timestamp::MAX).await
            }
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
    async fn codeowners(
        &self,
        key: &CodeownersKey,
    ) -> Result<Option<Versioned<KeptCodeowners>>, StoreError> {
        self.refuses()?;
        match self.fault {
            Fault::HoldsAnUnwrittenCodeowners => Ok(Some(Versioned {
                value: codeowned(("herald", None), "held", NOON)?,
                version: Version(1),
            })),
            Fault::ForgetsTheDefaultBranchCodeowners if key.reference.is_none() => Ok(None),
            Fault::ForgetsALongBranchCodeowners
                if key
                    .reference
                    .as_ref()
                    .is_some_and(|reference| reference.len() > LONG_BRANCH) =>
            {
                Ok(None)
            }
            Fault::SharesCodeowners => {
                let shared = CodeownersKey {
                    source: SourceName::new("herald"),
                    ..key.clone()
                };
                self.inner.codeowners(&shared).await
            }
            _ => self.inner.codeowners(key).await,
        }
    }

    async fn put_codeowners(
        &self,
        kept: &KeptCodeowners,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.refuses()?;
        let current = self
            .inner
            .codeowners(&kept.key)
            .await?
            .map(|read| read.version);
        match self.fault {
            Fault::OverwritesAnyCodeownersVersion => self.inner.put_codeowners(kept, current).await,
            Fault::KeepsTheFirstCodeowners if expected.is_some() && expected == current => {
                Ok(Written::Written(Version(9)))
            }
            _ => self.inner.put_codeowners(kept, expected).await,
        }
    }

    async fn base(&self, key: &BaseKey) -> Result<Option<Versioned<KeptBase>>, StoreError> {
        self.refuses()?;
        match self.fault {
            Fault::HoldsAnUnwrittenBase => Ok(Some(Versioned {
                value: based(("herald", "main"), "held", NOON)?,
                version: Version(1),
            })),
            Fault::ForgetsALongBranchBase if key.branch.len() > LONG_BRANCH => Ok(None),
            Fault::SharesBases => {
                let shared = BaseKey {
                    source: SourceName::new("herald"),
                    ..key.clone()
                };
                self.inner.base(&shared).await
            }
            _ => self.inner.base(key).await,
        }
    }

    async fn put_base(
        &self,
        kept: &KeptBase,
        expected: Option<Version>,
    ) -> Result<Written, StoreError> {
        self.refuses()?;
        let current = self.inner.base(&kept.key).await?.map(|read| read.version);
        match self.fault {
            Fault::OverwritesAnyBaseVersion => self.inner.put_base(kept, current).await,
            _ => self.inner.put_base(kept, expected).await,
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
        (
            Fault::HoldsAnUnwrittenCodeowners,
            "a new store holds a CODEOWNERS file",
        ),
        (
            Fault::OverwritesAnyCodeownersVersion,
            "a CODEOWNERS file was written over a version it did not expect",
        ),
        (
            Fault::KeepsTheFirstCodeowners,
            "a CODEOWNERS file was not the one written last under its key",
        ),
        (
            Fault::SharesCodeowners,
            "a CODEOWNERS file was not the one written last under its key",
        ),
        (
            Fault::ExpiresNoCodeowners,
            "a CODEOWNERS file read before the expiry was kept",
        ),
        (
            Fault::ExpiresEveryCodeowners,
            "a CODEOWNERS file read at the expiry was forgotten",
        ),
        (
            Fault::ForgetsTheDefaultBranchCodeowners,
            "a CODEOWNERS file read at the expiry was forgotten",
        ),
        (
            Fault::ForgetsALongBranchCodeowners,
            "a CODEOWNERS file of a long branch name was not kept",
        ),
    ]);
}

#[test]
fn a_store_that_breaks_its_promises_on_base_branches_is_refused() {
    refused(&[
        (
            Fault::HoldsAnUnwrittenBase,
            "a new store holds a base branch",
        ),
        (
            Fault::OverwritesAnyBaseVersion,
            "a base branch was written over a version it did not expect",
        ),
        (
            Fault::SharesBases,
            "a base branch was not the one written last under its key",
        ),
        (
            Fault::ExpiresNoBases,
            "a base branch read before the expiry was kept",
        ),
        (
            Fault::ExpiresEveryBase,
            "a base branch read at the expiry was forgotten",
        ),
        (
            Fault::ForgetsALongBranchBase,
            "a base branch of a long name was not kept",
        ),
    ]);
}
