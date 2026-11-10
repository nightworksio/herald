use futures_util::future::join_all;
use herald_core::{Cursor, Entry, Facts, SourceName, Step};
use herald_ports::{
    Admitted, Announcements, Cursors, Deliveries, Ledger, StoreError, Version, Written,
};

use super::{Checks, GITHUB, NOON, announced, key};

/// How many writers race for one record.
const RACERS: usize = 4;

/// How many of `answers` wrote.
fn won(answers: Vec<Result<Written, StoreError>>) -> Result<usize, StoreError> {
    let answers = answers.into_iter().collect::<Result<Vec<_>, _>>()?;
    Ok(answers
        .iter()
        .filter(|written| matches!(written, Written::Written(_)))
        .count())
}

/// Whether, of writers racing to make an entry and then to replace it at
/// one version, exactly one writes each time: what a ledger keeping its
/// entries with compare-and-set promises.
pub(super) async fn entries<S: Ledger>(store: &S, checks: &mut Checks) -> Result<(), StoreError> {
    let entry = Entry::new(key("raced")).attempted(Step::Post, NOON);
    let made = join_all((0..RACERS).map(|_| store.put_entry(&entry, None))).await;
    checks.ensure(
        won(made)? == 1,
        "of writers racing to make an entry, other than one wrote it",
    );
    let version = store.entry(&entry.key).await?.map(|kept| kept.version);
    let landed = entry.landed(Step::Post, Facts::default());
    let replaced = join_all((0..RACERS).map(|_| store.put_entry(&landed, version))).await;
    checks.ensure(
        won(replaced)? == 1,
        "of writers racing to replace an entry at one version, other than one wrote it",
    );
    Ok(())
}

/// Whether, of writers racing to make a cursor and then to replace it at
/// one version, exactly one writes each time.
pub(super) async fn cursors<S: Cursors>(store: &S, checks: &mut Checks) -> Result<(), StoreError> {
    let source = SourceName::new("raced");
    let cursor = Cursor::default();
    let made = join_all((0..RACERS).map(|_| store.put_cursor(&source, &cursor, None))).await;
    checks.ensure(
        won(made)? == 1,
        "of writers racing to make a cursor, other than one wrote it",
    );
    let version: Option<Version> = store.cursor(&source).await?.map(|kept| kept.version);
    let replaced = join_all((0..RACERS).map(|_| store.put_cursor(&source, &cursor, version))).await;
    checks.ensure(
        won(replaced)? == 1,
        "of writers racing to replace a cursor at one version, other than one wrote it",
    );
    Ok(())
}

/// Whether, of writers racing to record what a project last announced and
/// then to replace it at one version, exactly one writes each time.
pub(super) async fn latest<S: Announcements>(
    store: &S,
    checks: &mut Checks,
) -> Result<(), StoreError> {
    let first = announced("raced", "1.0.0");
    let made = join_all((0..RACERS).map(|_| store.put_latest(&first, None))).await;
    checks.ensure(
        won(made)? == 1,
        "of writers racing to record what a project last announced, other than one wrote it",
    );
    let version = store.latest(&first.project).await?.map(|kept| kept.version);
    let second = announced("raced", "1.1.0");
    let replaced = join_all((0..RACERS).map(|_| store.put_latest(&second, version))).await;
    checks.ensure(
        won(replaced)? == 1,
        "of writers racing to replace what a project last announced at one version, other than one wrote it",
    );
    Ok(())
}

/// Whether, of runs admitting one delivery at once, exactly one admits it
/// fresh.
pub(super) async fn admissions<S: Deliveries>(
    store: &S,
    checks: &mut Checks,
) -> Result<(), StoreError> {
    let admitted = join_all((0..RACERS).map(|_| store.admit(GITHUB, "raced", NOON)))
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    checks.ensure(
        admitted
            .iter()
            .filter(|admitted| **admitted == Admitted::Fresh)
            .count()
            == 1,
        "of runs admitting one delivery at once, other than one admitted it fresh",
    );
    Ok(())
}
