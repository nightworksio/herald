use herald_core::{Reran, RerunRecord, RunKey};
use herald_ports::{Actions, StoreError, Written};

use super::{Checks, NOON};

/// The job `job` of the run `run` of `o/r` on the head `head`.
pub(super) fn run(run: &str, job: &str, head: &str) -> RunKey {
    RunKey {
        api: "https://api.github.com".to_owned(),
        repository: "o/r".to_owned(),
        run: run.to_owned(),
        job: job.to_owned(),
        head: head.to_owned(),
    }
}

/// The record of a re-run of `key` that came to `outcome`.
pub(super) fn action(key: RunKey, outcome: Reran) -> RerunRecord {
    RerunRecord {
        key,
        operator: "octocat".to_owned(),
        credential: "HERALD_RERUN".to_owned(),
        at: NOON,
        reruns: 0,
        outcome,
    }
}

/// Whether the record of each re-run is kept per job and head, written as
/// new only once, so two applies racing never both make the call.
pub(super) async fn actions<S: Actions>(store: &S, checks: &mut Checks) -> Result<(), StoreError> {
    let key = run("7", "70", "abc");
    checks.ensure(
        store.action(&key).await?.is_none(),
        "a new store holds the record of a re-run",
    );
    let calling = action(key.clone(), Reran::Calling);
    let version = checks.written(store.put_action(&calling, None).await?);
    checks.ensure(
        store
            .action(&key)
            .await?
            .map(|read| (read.value, read.version))
            == Some((calling.clone(), version)),
        "the record of a re-run read back otherwise",
    );
    checks.ensure(
        store.put_action(&calling, None).await? == Written::Conflict,
        "the record of a re-run was written as new over one that exists",
    );
    let rerun = action(key.clone(), Reran::Rerun);
    let next = checks.written(store.put_action(&rerun, Some(version)).await?);
    checks.ensure(
        store.put_action(&calling, Some(version)).await? == Written::Conflict,
        "the record of a re-run was written over a stale version",
    );
    checks.ensure(
        store
            .action(&key)
            .await?
            .map(|read| (read.value, read.version))
            == Some((rerun, next)),
        "the record of a re-run did not read back as last written",
    );
    let moved = run("7", "70", "def");
    checks.ensure(
        store.action(&moved).await?.is_none(),
        "the record of a re-run on one head was read for another",
    );
    let other = run("7", "71", "abc");
    checks.ensure(
        store.action(&other).await?.is_none(),
        "the record of a re-run of one job was read for another",
    );
    Ok(())
}
