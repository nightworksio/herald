use herald_core::merging::{GestureKey, GestureRecord, HandOff, HandedOff};
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

/// The gesture on pull request 7 of `o/r` the forge names by the id `id`.
pub(super) fn gestured(id: u64) -> GestureKey {
    GestureKey {
        api: "https://api.github.com".to_owned(),
        repository: "o/r".to_owned(),
        number: 7,
        gesture: id,
    }
}

/// The record of the gesture `key`, its head first seen and, where
/// `outcome` names one, its hand-off.
pub(super) fn gesture(key: GestureKey, outcome: Option<HandedOff>) -> GestureRecord {
    GestureRecord {
        key,
        by: "w".to_owned(),
        at: NOON,
        queue: "main".to_owned(),
        seen: "abc".to_owned(),
        seen_at: NOON,
        hand_off: outcome.map(|outcome| HandOff {
            head: "abc".to_owned(),
            base: "def".to_owned(),
            credential: "HERALD_MERGE".to_owned(),
            at: NOON,
            outcome,
        }),
        taken_out: Vec::new(),
        own: herald_core::merging::Own::default(),
    }
}

/// Whether the record of each gesture that queued a pull request is kept
/// per gesture, written as new only once, so two runs racing never both
/// hand it over.
pub(super) async fn gestures<S: Actions>(store: &S, checks: &mut Checks) -> Result<(), StoreError> {
    let key = gestured(0);
    checks.ensure(
        store.gesture(&key).await?.is_none(),
        "a new store holds the record of a gesture",
    );
    let calling = gesture(key.clone(), Some(HandedOff::Calling));
    let version = checks.written(store.put_gesture(&calling, None).await?);
    checks.ensure(
        store.put_gesture(&calling, None).await? == Written::Conflict,
        "the record of a gesture was written as new over one that exists",
    );
    let queued = gesture(key.clone(), Some(HandedOff::Queued));
    let next = checks.written(store.put_gesture(&queued, Some(version)).await?);
    checks.ensure(
        store.put_gesture(&calling, Some(version)).await? == Written::Conflict,
        "the record of a gesture was written over a stale version",
    );
    checks.ensure(
        store
            .gesture(&key)
            .await?
            .map(|read| (read.value, read.version))
            == Some((queued, next)),
        "the record of a gesture did not read back as last written",
    );
    checks.ensure(
        store.gesture(&gestured(1)).await?.is_none(),
        "the record of one gesture was read for another id",
    );
    let again = GestureKey {
        repository: "o/s".to_owned(),
        ..key
    };
    checks.ensure(
        store.gesture(&again).await?.is_none(),
        "the record of one gesture was read for one in another repository with its id",
    );
    Ok(())
}
