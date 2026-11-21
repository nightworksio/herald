use herald_core::AdvisedReports;
use herald_ports::{Recalls, StoreError, Written};

use super::Checks;

/// The record naming `names`.
pub(super) fn named(names: &[&str]) -> AdvisedReports {
    AdvisedReports {
        reports: names.iter().map(|name| (*name).to_owned()).collect(),
    }
}

/// Whether the names of the reports whose living posts were sent
/// advisories are kept as one record, written with compare-and-set.
pub(super) async fn recalls<S: Recalls>(store: &S, checks: &mut Checks) -> Result<(), StoreError> {
    checks.ensure(
        store.advised().await?.is_none(),
        "a new store holds names of reports whose living posts were sent advisories",
    );
    let first = named(&["daily"]);
    let version = checks.written(store.put_advised(&first, None).await?);
    let read = store.advised().await?;
    checks.ensure(
        read.as_ref().map(|read| (&read.value, read.version)) == Some((&first, version)),
        "the names of reports whose living posts were sent advisories read back otherwise",
    );
    checks.ensure(
        store.put_advised(&named(&["weekly"]), None).await? == Written::Conflict,
        "the names of reports were written as new over a record that exists",
    );
    let both = named(&["daily", "weekly"]);
    let next = checks.written(store.put_advised(&both, Some(version)).await?);
    checks.ensure(
        store.put_advised(&first, Some(version)).await? == Written::Conflict,
        "the names of reports were written over a stale version",
    );
    let read = store.advised().await?;
    checks.ensure(
        read.as_ref().map(|read| (&read.value, read.version)) == Some((&both, next)),
        "the names of reports did not read back as last written",
    );
    Ok(())
}
