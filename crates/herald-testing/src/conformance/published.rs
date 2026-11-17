use herald_core::PublishedKeys;
use herald_ports::{Published, StoreError, Version, Written};

use super::Checks;

fn set(sequence: u64, keys: &[&str]) -> PublishedKeys {
    PublishedKeys {
        sequence,
        keys: keys.iter().map(|key| (*key).to_owned()).collect(),
    }
}

/// Whether the key set the instance published is kept, written with
/// compare-and-set, and read back as written last.
pub(super) async fn published<S: Published>(
    store: &S,
    checks: &mut Checks,
) -> Result<(), StoreError> {
    checks.ensure(
        store.published_keys().await?.is_none(),
        "a new store keeps a published key set",
    );
    let first = set(1, &["z6Mkold"]);
    let at = checks.written(store.put_published_keys(&first, None).await?);
    checks.ensure(
        store.put_published_keys(&first, None).await? == Written::Conflict
            && store.put_published_keys(&first, Some(Version(0))).await? == Written::Conflict,
        "a published key set was written over a version it did not expect",
    );
    let next = set(2, &["z6Mknew", "z6Mkold"]);
    checks.written(store.put_published_keys(&next, Some(at)).await?);
    checks.ensure(
        store
            .published_keys()
            .await?
            .is_some_and(|kept| kept.value == next),
        "a published key set read back was not the one written last",
    );
    Ok(())
}
