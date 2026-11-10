use herald_ports::{Kv, KvError, Pair, Stamped};

use super::{Checks, Failure};

impl From<KvError> for Failure {
    fn from(error: KvError) -> Self {
        Self(format!("the key-value store failed: {error}"))
    }
}

/// The first part of every key the suite writes, so it shares a store with
/// no other test's records.
const PREFIX: &str = "kv-conformance";

fn key(parts: &[&str]) -> Vec<String> {
    std::iter::once(PREFIX)
        .chain(parts.iter().copied())
        .map(str::to_owned)
        .collect()
}

/// A value no text holds, so a store that keeps values as text fails.
const VALUE: [u8; 4] = [0, 0xff, b'\n', 0x80];

/// Whether `kv`, empty, keeps each value at its versionstamp, sets and
/// deletes only against the versionstamp it was given, never takes a
/// versionstamp back, and lists only what lies under a prefix. Every
/// key-value store passes this suite, and so does [`crate::FakeKv`].
///
/// # Errors
///
/// The first way the store fails, as a sentence.
pub async fn kv_conformance<K: Kv>(kv: &K) -> Result<(), Failure> {
    let mut checks = Checks::default();
    let kept = key(&["kept", "one"]);
    checks.ensure(kv.get(&kept).await?.is_none(), "a key never set was found");
    let Some(first) = kv.set_if(&kept, VALUE.to_vec(), None).await? else {
        return Err(Failure("a key was not set where it was absent".to_owned()));
    };
    checks.ensure(
        kv.get(&kept).await?
            == Some(Stamped {
                value: VALUE.to_vec(),
                versionstamp: first.clone(),
            }),
        "a value set was not read back as set, at its versionstamp",
    );
    checks.ensure(
        kv.set_if(&kept, b"again".to_vec(), None).await?.is_none(),
        "a key was set as absent where it holds a value",
    );
    let Some(second) = kv
        .set_if(&kept, b"second".to_vec(), Some(first.clone()))
        .await?
    else {
        return Err(Failure(
            "a value was not set against its versionstamp".to_owned(),
        ));
    };
    checks.ensure(second != first, "a value set again kept its versionstamp");
    checks.ensure(
        kv.set_if(&kept, b"stale".to_vec(), Some(first.clone()))
            .await?
            .is_none(),
        "a value was set against a versionstamp it no longer has",
    );
    checks.ensure(
        !kv.delete_if(&kept, first).await?,
        "a key was deleted against a versionstamp it no longer has",
    );
    checks.ensure(
        kv.get(&kept).await?.map(|stamped| stamped.value) == Some(b"second".to_vec()),
        "a refused write or delete changed the value",
    );
    checks.ensure(
        kv.delete_if(&kept, second.clone()).await?,
        "a key was not deleted against its versionstamp",
    );
    checks.ensure(
        kv.get(&kept).await?.is_none(),
        "a deleted key was still found",
    );
    checks.ensure(
        kv.list(&key(&["kept"])).await?.is_empty(),
        "a deleted key was still listed",
    );
    checks.ensure(
        !kv.delete_if(&kept, second.clone()).await?,
        "a deleted key was deleted again",
    );
    let again = kv.set_if(&kept, b"again".to_vec(), None).await?;
    checks.ensure(
        again.is_some_and(|again| again != second),
        "a deleted key was not set again where it was absent, at a new versionstamp",
    );
    listed(kv, &mut checks).await?;
    checks.done()
}

async fn listed<K: Kv>(kv: &K, checks: &mut Checks) -> Result<(), KvError> {
    let under = [
        (key(&["listed", "a"]), b"a".to_vec()),
        (key(&["listed", "b", "c"]), b"c".to_vec()),
    ];
    for (key, value) in under.iter().chain(&[
        (key(&["listedness"]), b"beside".to_vec()),
        (
            ["elsewhere", "listed", "a"].map(str::to_owned).to_vec(),
            b"elsewhere".to_vec(),
        ),
    ]) {
        kv.set_if(key, value.clone(), None).await?;
    }
    let mut pairs = kv.list(&key(&["listed"])).await?;
    pairs.sort_by(|one, other| one.key.cmp(&other.key));
    let expected: Vec<Pair> = under
        .into_iter()
        .map(|(key, value)| Pair { key, value })
        .collect();
    checks.ensure(
        pairs == expected,
        "a prefix did not list every key under it, with its value, and nothing else",
    );
    checks.ensure(
        kv.list(&key(&["nowhere"])).await?.is_empty(),
        "a prefix nothing lies under listed a key",
    );
    Ok(())
}

#[cfg(test)]
mod tests;
