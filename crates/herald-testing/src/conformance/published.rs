use herald_core::Action;
use std::collections::BTreeMap;

use herald_core::{
    AuditDay, Audited, DestinationName, Feed, PeerKeys, Publication, PublishedKeys, Window,
};
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

/// Whether the static signed log's record is kept, written with
/// compare-and-set, and read back as written last, every field whole.
pub(super) async fn publication<S: Published>(
    store: &S,
    checks: &mut Checks,
) -> Result<(), StoreError> {
    checks.ensure(
        store.publication().await?.is_none(),
        "a new store keeps a static log's record",
    );
    let begun = Publication {
        sequence: 1,
        published: None,
        files: BTreeMap::new(),
        commit: None,
    };
    let at = checks.written(store.put_publication(&begun, None).await?);
    checks.ensure(
        store.put_publication(&begun, None).await? == Written::Conflict
            && store.put_publication(&begun, Some(Version(0))).await? == Written::Conflict,
        "a static log's record was written over a version it did not expect",
    );
    let written = Publication {
        sequence: 2,
        published: Some(Timestamp::constant(1_800_000_000, 0)),
        files: BTreeMap::from([(
            "herald/v1/feeds/releases".to_owned(),
            "sha-256=:AAAA:".to_owned(),
        )]),
        commit: Some("0123abcd".to_owned()),
    };
    checks.written(store.put_publication(&written, Some(at)).await?);
    checks.ensure(
        store
            .publication()
            .await?
            .is_some_and(|kept| kept.value == written),
        "a static log's record read back was not the one written last",
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

fn day_of(peer: &str, day: &str, reason: &str) -> AuditDay {
    AuditDay {
        peer: peer.to_owned(),
        day: day.to_owned(),
        entries: vec![Audited {
            at: Timestamp::UNIX_EPOCH,
            peer: Some(peer.to_owned()),
            keyid: None,
            action: Action::Push,
            resource: "sources.releases".to_owned(),
            outcome: 202,
            reason: reason.to_owned(),
            evidence: std::collections::BTreeMap::from([(
                "iss".to_owned(),
                "https://issuer.example".to_owned(),
            )]),
            identity: Some("github.com/1/v1".to_owned()),
            digest: Some("sha-256=:YWI=:".to_owned()),
        }],
        more: 0,
    }
}

/// Whether each peer's days are kept apart from every other peer's and
/// day's, written with compare-and-set, listed, and forgotten
/// once before the day expired; and each peer's window apart.
pub(super) async fn audits<S: Published>(store: &S, checks: &mut Checks) -> Result<(), StoreError> {
    let (upstream, other) = ("upstream", "other");
    checks.ensure(
        store.audit(upstream, "2027-01-02").await?.is_none(),
        "a new store keeps a peer's audit",
    );
    checks.ensure(
        store.audits(upstream).await?.is_empty(),
        "a new store lists a peer's audits",
    );
    let first = day_of(upstream, "2027-01-02", "taken");
    let at = checks.written(store.put_audit(&first, None).await?);
    checks.ensure(
        store.put_audit(&first, None).await? == Written::Conflict
            && store.put_audit(&first, Some(Version(0))).await? == Written::Conflict,
        "a peer's audit was written over a version it did not expect",
    );
    let next = day_of(upstream, "2027-01-02", "taken again");
    checks.written(store.put_audit(&next, Some(at)).await?);
    let earlier = day_of(upstream, "2027-01-01", "taken first");
    checks.written(store.put_audit(&earlier, None).await?);
    checks.written(
        store
            .put_audit(&day_of(other, "2027-01-02", "other"), None)
            .await?,
    );
    checks.ensure(
        store
            .audit(upstream, "2027-01-02")
            .await?
            .is_some_and(|kept| kept.value == next)
            && store.audit(other, "2027-01-01").await?.is_none(),
        "a peer's audit read back was not the one written last for its day",
    );
    let mut listed = store.audits(upstream).await?;
    listed.sort_by(|one, other| one.day.cmp(&other.day));
    checks.ensure(
        listed == vec![earlier, next.clone()],
        "a peer's audits were not listed apart from other peers'",
    );
    store.expire_audits("2027-01-02").await?;
    checks.ensure(
        store.audits(upstream).await? == vec![next] && store.audits(other).await?.len() == 1,
        "an audit before the day expired was kept, or one from it forgotten",
    );
    checks.ensure(
        store.window(upstream).await?.is_none(),
        "a new store keeps a peer's window",
    );
    let window = Window {
        start: super::NOON,
        count: 1,
        refused: None,
        held: None,
    };
    let at = checks.written(store.put_window(upstream, &window, None).await?);
    checks.ensure(
        store
            .put_window(upstream, &window, Some(Version(0)))
            .await?
            == Written::Conflict,
        "a peer's window was written over a version it did not expect",
    );
    let counted = Window {
        start: super::NOON,
        count: 2,
        refused: Some(herald_core::Refusals {
            start: super::NOON,
            count: 3,
        }),
        held: Some(super::NOON),
    };
    checks.written(store.put_window(upstream, &counted, Some(at)).await?);
    checks.ensure(
        store
            .window(upstream)
            .await?
            .is_some_and(|kept| kept.value == counted)
            && store.window(other).await?.is_none(),
        "a peer's window read back was not its own written last",
    );
    Ok(())
}
