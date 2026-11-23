//! The pull requests herald keeps in kept mode, where each source's
//! reconcile stands (ADR-0061), each CODEOWNERS file a source read
//! (ADR-0074), and each release base branch's checks it read (ADR-0083).

use std::future::{Future, ready};

use herald_core::{
    BaseKey, CodeownersKey, KeptBase, KeptCodeowners, KeptPull, PullKey, Reconciled, RepositoryKey,
    SourceName,
};
use jiff::Timestamp;

use super::{Held, StoreError, Unkept, Version, Versioned, Written};

/// The pull requests herald keeps, one record per pull request written
/// with compare-and-set, where each source's reconcile stands (ADR-0061),
/// each CODEOWNERS file a source read with its validator (ADR-0074), and
/// each base branch's checks with their validators (ADR-0083).
pub trait Pulls: Sync {
    /// The pull request keyed `key`, where it is kept.
    fn pull(
        &self,
        key: &PullKey,
    ) -> impl Future<Output = Result<Option<Versioned<KeptPull>>, StoreError>> + Send;

    /// Every pull request of `repository` kept, open or closed.
    fn pulls_of(
        &self,
        repository: &RepositoryKey,
    ) -> impl Future<Output = Result<Vec<Versioned<KeptPull>>, StoreError>> + Send;

    /// Writes `kept` under its key if the stored one is at `expected`, or
    /// absent when `expected` is `None`.
    fn put_pull(
        &self,
        kept: &KeptPull,
        expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send;

    /// Forgets every pull request that closed before `before`, and every
    /// CODEOWNERS file and base branch last read in full before it.
    fn expire_pulls(
        &self,
        before: Timestamp,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Where the reconcile of `source` stands.
    fn reconciled(
        &self,
        source: &SourceName,
    ) -> impl Future<Output = Result<Option<Versioned<Reconciled>>, StoreError>> + Send;

    /// Writes where the reconcile of `source` stands if the stored record
    /// is at `expected`, or absent when `expected` is `None`.
    fn put_reconciled(
        &self,
        source: &SourceName,
        reconciled: &Reconciled,
        expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send;
    /// The CODEOWNERS file keyed `key` as a source read it, where kept.
    fn codeowners(
        &self,
        key: &CodeownersKey,
    ) -> impl Future<Output = Result<Option<Versioned<KeptCodeowners>>, StoreError>> + Send;

    /// Writes `kept` under its key if the stored one is at `expected`, or
    /// absent when `expected` is `None`.
    fn put_codeowners(
        &self,
        kept: &KeptCodeowners,
        expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send;

    /// The base branch keyed `key` as a source last read its checks, where
    /// kept.
    fn base(
        &self,
        key: &BaseKey,
    ) -> impl Future<Output = Result<Option<Versioned<KeptBase>>, StoreError>> + Send;

    /// Writes `kept` under its key if the stored one is at `expected`, or
    /// absent when `expected` is `None`.
    fn put_base(
        &self,
        kept: &KeptBase,
        expected: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send;
}

impl Pulls for Unkept {
    fn pull(
        &self,
        _: &PullKey,
    ) -> impl Future<Output = Result<Option<Versioned<KeptPull>>, StoreError>> + Send {
        ready(Err(Held::Pulls.unsupported()))
    }

    fn pulls_of(
        &self,
        _: &RepositoryKey,
    ) -> impl Future<Output = Result<Vec<Versioned<KeptPull>>, StoreError>> + Send {
        ready(Err(Held::Pulls.unsupported()))
    }

    fn put_pull(
        &self,
        _: &KeptPull,
        _: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send {
        ready(Err(Held::Pulls.unsupported()))
    }

    fn expire_pulls(&self, _: Timestamp) -> impl Future<Output = Result<(), StoreError>> + Send {
        ready(Err(Held::Pulls.unsupported()))
    }

    fn reconciled(
        &self,
        _: &SourceName,
    ) -> impl Future<Output = Result<Option<Versioned<Reconciled>>, StoreError>> + Send {
        ready(Err(Held::Pulls.unsupported()))
    }

    fn put_reconciled(
        &self,
        _: &SourceName,
        _: &Reconciled,
        _: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send {
        ready(Err(Held::Pulls.unsupported()))
    }

    fn codeowners(
        &self,
        _: &CodeownersKey,
    ) -> impl Future<Output = Result<Option<Versioned<KeptCodeowners>>, StoreError>> + Send {
        ready(Err(Held::Pulls.unsupported()))
    }

    fn put_codeowners(
        &self,
        _: &KeptCodeowners,
        _: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send {
        ready(Err(Held::Pulls.unsupported()))
    }

    fn base(
        &self,
        _: &BaseKey,
    ) -> impl Future<Output = Result<Option<Versioned<KeptBase>>, StoreError>> + Send {
        ready(Err(Held::Pulls.unsupported()))
    }

    fn put_base(
        &self,
        _: &KeptBase,
        _: Option<Version>,
    ) -> impl Future<Output = Result<Written, StoreError>> + Send {
        ready(Err(Held::Pulls.unsupported()))
    }
}
