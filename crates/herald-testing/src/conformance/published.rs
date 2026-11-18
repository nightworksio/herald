use herald_core::{DestinationName, Feed, PeerKeys, PublishedKeys};
use herald_ports::{Published, StoreError, Version, Written};
use jiff::Timestamp;
use serde_json::json;

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

/// Whether each feed's envelopes are kept apart from every other feed's,
/// written with compare-and-set, and read back as written last.
pub(super) async fn feeds<S: Published>(store: &S, checks: &mut Checks) -> Result<(), StoreError> {
    let (releases, tags) = (
        DestinationName::new("releases"),
        DestinationName::new("tags"),
    );
    checks.ensure(
        store.feed(&releases).await?.is_none(),
        "a new store keeps a feed",
    );
    let first = Feed::default().entered(json!({ "identity": "forge.example/1/v1", "n": 1.5 }), 3);
    let at = checks.written(store.put_feed(&releases, &first, None).await?);
    checks.ensure(
        store.put_feed(&releases, &first, None).await? == Written::Conflict
            && store.put_feed(&releases, &first, Some(Version(0))).await? == Written::Conflict,
        "a feed was written over a version it did not expect",
    );
    checks.ensure(
        store.feed(&tags).await?.is_none(),
        "a feed was kept as another feed's",
    );
    let next = first.entered(json!({ "identity": "forge.example/1/v2" }), 3);
    checks.written(store.put_feed(&releases, &next, Some(at)).await?);
    checks.ensure(
        store
            .feed(&releases)
            .await?
            .is_some_and(|kept| kept.value == next),
        "a feed read back was not the one written last",
    );
    Ok(())
}

/// Whether each peer's key set is kept apart from every other peer's,
/// written with compare-and-set, and read back as written last.
pub(super) async fn peers<S: Published>(store: &S, checks: &mut Checks) -> Result<(), StoreError> {
    let (upstream, other) = ("upstream", "other");
    checks.ensure(
        store.peer_keys(upstream).await?.is_none(),
        "a new store keeps a peer's key set",
    );
    let first = PeerKeys {
        pinned: vec!["z6Mkold".to_owned()],
        sequence: 1,
        keys: vec!["z6Mkold".to_owned()],
        read: Timestamp::UNIX_EPOCH,
        refresh: 86_400,
    };
    let at = checks.written(store.put_peer_keys(upstream, &first, None).await?);
    checks.ensure(
        store.put_peer_keys(upstream, &first, None).await? == Written::Conflict
            && store
                .put_peer_keys(upstream, &first, Some(Version(0)))
                .await?
                == Written::Conflict,
        "a peer's key set was written over a version it did not expect",
    );
    checks.ensure(
        store.peer_keys(other).await?.is_none(),
        "a peer's key set was kept as another peer's",
    );
    let next = PeerKeys {
        sequence: 2,
        keys: vec!["z6Mknew".to_owned(), "z6Mkold".to_owned()],
        ..first
    };
    checks.written(store.put_peer_keys(upstream, &next, Some(at)).await?);
    checks.ensure(
        store
            .peer_keys(upstream)
            .await?
            .is_some_and(|kept| kept.value == next),
        "a peer's key set read back was not the one written last",
    );
    checks.ensure(
        store
            .peer_keys(upstream)
            .await?
            .is_some_and(|kept| kept.value.keys.len() == 2),
        "a peer's key set read back did not list the keys written last",
    );
    Ok(())
}
