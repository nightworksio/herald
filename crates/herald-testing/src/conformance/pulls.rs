use herald_core::{
    Checks, Codeowners, CodeownersKey, Dialect, KeptCodeowners, KeptPull, Mergeable, Pull, PullKey,
    Reconciled, RepositoryKey, Review, SourceName, Validators, Visibility,
};
use herald_ports::{Pulls, StoreError, Version, Written};

use super::{Checks as Checked, NOON};

/// The repository of id `id` on github.com.
pub(super) fn repository(id: &str) -> RepositoryKey {
    RepositoryKey {
        host: "github.com".to_owned(),
        id: id.to_owned(),
    }
}

/// Pull request `number` of the repository of id `id`, titled `title`
/// and read at noon.
pub(super) fn kept(id: &str, number: u64, title: &str) -> KeptPull {
    let pull = Pull {
        project: "herald".to_owned(),
        repository: "nightworksio/herald".to_owned(),
        visibility: Visibility::Public,
        number,
        title: title.to_owned(),
        url: format!("https://github.com/nightworksio/herald/pull/{number}"),
        author: Some("wessel".to_owned()),
        bot: false,
        draft: false,
        labels: Vec::new(),
        opened: NOON,
        updated: NOON,
        review: Review::Unasked,
        reviewers: Vec::new(),
        checks: Checks::Absent,
        mergeable: Mergeable::Unknown,
        queued: false,
        forge: None,
        head: Some("abc123".to_owned()),
        unread: false,
        base: None,
        files: None,
        areas: Vec::new(),
        flips: Vec::new(),
    };
    let key = PullKey {
        repository: repository(id),
        number,
    };
    KeptPull::read(None, key, pull, NOON)
}

/// Whether pull requests are kept by key, written with compare-and-set,
/// listed by repository alone, and forgotten only once closed before the
/// expiry.
pub(super) async fn pulls<S: Pulls>(store: &S, checks: &mut Checked) -> Result<(), StoreError> {
    let first = kept("42", 7, "first");
    checks.ensure(
        store.pull(&first.key).await?.is_none()
            && store.pulls_of(&repository("42")).await?.is_empty(),
        "a new store holds a pull request",
    );
    let version = checks.written(store.put_pull(&first, None).await?);
    checks.ensure(
        store.put_pull(&first, None).await? == Written::Conflict,
        "a pull request was written as new over one that exists",
    );
    checks.ensure(
        store.put_pull(&first, Some(Version(0))).await? == Written::Conflict,
        "a pull request was written over a version it did not expect",
    );
    let moved = kept("42", 7, "moved");
    checks.written(store.put_pull(&moved, Some(version)).await?);
    checks.ensure(
        store
            .pull(&first.key)
            .await?
            .is_some_and(|read| read.value == moved),
        "a pull request was not the one written last under its key",
    );
    let sibling = kept("42", 8, "sibling").vanished(NOON);
    let other = kept("4", 9, "other").vanished(NOON + jiff::SignedDuration::from_hours(1));
    checks.written(store.put_pull(&sibling, None).await?);
    checks.written(store.put_pull(&other, None).await?);
    let mut listed: Vec<KeptPull> = store
        .pulls_of(&repository("42"))
        .await?
        .into_iter()
        .map(|read| read.value)
        .collect();
    listed.sort_by_key(|kept| kept.key.number);
    checks.ensure(
        listed == [moved.clone(), sibling.clone()],
        "a repository's pull requests were not its own and every one",
    );
    store
        .expire_pulls(NOON + jiff::SignedDuration::from_hours(1))
        .await?;
    checks.ensure(
        store.pull(&sibling.key).await?.is_none(),
        "a pull request closed before the expiry was kept",
    );
    checks.ensure(
        store.pull(&moved.key).await?.is_some() && store.pull(&other.key).await?.is_some(),
        "an open pull request, or one closed at the expiry, was forgotten",
    );
    Ok(())
}

/// Whether where each source's reconcile stands is kept by source, and
/// written with compare-and-set.
pub(super) async fn reconciled<S: Pulls>(
    store: &S,
    checks: &mut Checked,
) -> Result<(), StoreError> {
    let (source, other) = (SourceName::new("herald"), SourceName::new("other"));
    let listed = Reconciled {
        repositories: std::collections::BTreeSet::new(),
        repository: Some(repository("42")),
        at: Some(NOON),
        validators: vec![Validators {
            etag: Some("\"1\"".to_owned()),
            last_modified: None,
        }],
    };
    checks.ensure(
        store.reconciled(&source).await?.is_none(),
        "a new store holds a reconcile",
    );
    let version = checks.written(store.put_reconciled(&source, &listed, None).await?);
    checks.ensure(
        store.put_reconciled(&source, &listed, None).await? == Written::Conflict,
        "a reconcile was written over a version it did not expect",
    );
    let moved = Reconciled {
        validators: Vec::new(),
        ..listed
    };
    checks.written(store.put_reconciled(&source, &moved, Some(version)).await?);
    checks.ensure(
        store
            .reconciled(&source)
            .await?
            .is_some_and(|read| read.value == moved)
            && store.reconciled(&other).await?.is_none(),
        "a reconcile was not the one written last for its source",
    );
    Ok(())
}

/// A CODEOWNERS file read at `reference` by the source `source`, kept as
/// read at `at` with the validator `etag`.
pub(super) fn codeowned(
    (source, reference): (&str, Option<&str>),
    etag: &str,
    at: jiff::Timestamp,
) -> Result<KeptCodeowners, StoreError> {
    let key = CodeownersKey {
        source: SourceName::new(source),
        repository: "o/r".to_owned(),
        reference: reference.map(str::to_owned),
    };
    let file = Codeowners::read(Dialect::GitHub, "* @o/all\n", 1);
    KeptCodeowners::of(key, ("CODEOWNERS", etag), &file, ((4096, 1), at))
        .ok_or_else(|| StoreError::corrupt("a one-line CODEOWNERS file is not kept"))
}

/// Whether each CODEOWNERS file a source read is kept by source,
/// repository and branch, written with compare-and-set, and forgotten
/// once last read in full before an expiry.
pub(super) async fn codeowners<S: Pulls>(
    store: &S,
    checks: &mut Checked,
) -> Result<(), StoreError> {
    let (first, last) = (
        NOON + jiff::SignedDuration::from_hours(2),
        NOON + jiff::SignedDuration::from_hours(3),
    );
    let read = codeowned(("herald", Some("main")), "\"1\"", first)?;
    checks.ensure(
        store.codeowners(&read.key).await?.is_none(),
        "a new store holds a CODEOWNERS file",
    );
    let version = checks.written(store.put_codeowners(&read, None).await?);
    checks.ensure(
        store.put_codeowners(&read, None).await? == Written::Conflict,
        "a CODEOWNERS file was written over a version it did not expect",
    );
    let moved = codeowned(("herald", Some("main")), "\"2\"", last)?;
    checks.written(store.put_codeowners(&moved, Some(version)).await?);
    let (other, default) = (
        codeowned(("other", Some("main")), "\"3\"", first)?,
        codeowned(("herald", None), "\"4\"", last)?,
    );
    checks.written(store.put_codeowners(&other, None).await?);
    checks.written(store.put_codeowners(&default, None).await?);
    checks.ensure(
        store
            .codeowners(&read.key)
            .await?
            .is_some_and(|kept| kept.value == moved)
            && store
                .codeowners(&other.key)
                .await?
                .is_some_and(|kept| kept.value == other),
        "a CODEOWNERS file was not the one written last under its key",
    );
    store.expire_pulls(last).await?;
    checks.ensure(
        store.codeowners(&other.key).await?.is_none(),
        "a CODEOWNERS file read before the expiry was kept",
    );
    checks.ensure(
        store.codeowners(&moved.key).await?.is_some()
            && store.codeowners(&default.key).await?.is_some(),
        "a CODEOWNERS file read at the expiry was forgotten",
    );
    Ok(())
}
