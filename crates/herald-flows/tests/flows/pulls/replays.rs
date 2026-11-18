//! A peer's answers and envelopes that could hold a feed still or turn it
//! back: an answer saying nothing changed read only once signed over the
//! entity tag asked for, and an envelope taken only where its proof is
//! newer than any taken of its identity.

use herald_core::Identity;
use herald_flows::{Stop, Sweep};
use herald_ports::Response;
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};

use super::{
    DISCOVERY, DOMAIN, FEED, FROM_PEER, OLD, ORIGIN, Peer, answered, config, cursor, envelope,
    listing, page, proved_at, pulled, release, swept,
};
use crate::harness::{Failing, NOON, healthy};
use crate::pulls::tombstones::tombstone;

const TAG: &str = "\"1.0.20\"";

/// `minutes` after an hour before noon, the round's time.
fn at(minutes: i64) -> Timestamp {
    NOON - SignedDuration::from_hours(1) + SignedDuration::from_mins(minutes)
}

/// The peer's release `identity`, its notes `body`, proved at `minutes`.
fn noted(identity: &str, body: &str, minutes: i64) -> Value {
    let mut released = envelope(identity, &[DISCOVERY]);
    released["attributes"]["signal"] = json!("release");
    released["body"] = json!(body);
    proved_at(&released, (DOMAIN, &OLD), at(minutes))
}

/// The origin's release `identity`, its notes `body`, relayed by the
/// peer, proved at `minutes`.
fn relayed(identity: &str, body: &str, minutes: i64) -> Value {
    let origin = "https://origin.example/.well-known/herald";
    let mut released = envelope(identity, &[origin, DISCOVERY]);
    released["attributes"]["signal"] = json!("release");
    released["body"] = json!(body);
    proved_at(&released, ("origin.example", &ORIGIN), at(minutes))
}

/// The identities a pull of `items` announced, edits marked.
fn pulling(store: &Failing, items: &[(u64, Value)]) -> Vec<String> {
    let config = config("", FROM_PEER);
    let Sweep::Polled { announced, .. } =
        pulled(&config, store, &Peer::answering(vec![page(items, &OLD)]))
    else {
        panic!("a pull");
    };
    announced
        .iter()
        .map(|fresh| {
            let edited = if fresh.event.edited() { " edited" } else { "" };
            format!("{}{edited}", fresh.event.identity().as_str())
        })
        .collect()
}

#[test]
fn an_unchanged_answer_is_read_only_once_signed_over_the_tag_the_pull_holds() {
    let config = config("", FROM_PEER);
    let store = healthy();
    let tagged = answered(
        FEED,
        (200, &listing(&[(1, release("a/1", &OLD))])),
        &OLD,
        Some(TAG),
    );
    pulled(&config, &store, &Peer::answering(vec![tagged]));
    assert_eq!(cursor(&store).unwrap().etag.as_deref(), Some(TAG));
    let unchanged = answered(FEED, (304, ""), &OLD, Some(TAG));
    assert_eq!(
        pulled(&config, &store, &Peer::answering(vec![unchanged])),
        Sweep::Unchanged
    );
    let unsigned = Response {
        status: 304,
        headers: vec![("etag".to_owned(), TAG.to_owned())],
        body: Vec::new(),
    };
    assert_eq!(
        pulled(&config, &store, &Peer::answering(vec![unsigned])),
        Sweep::Stopped(Stop::Source(
            "the message carries no signature tagged herald".to_owned()
        ))
    );
    let retagged = answered(FEED, (304, ""), &OLD, Some("\"0.0.20\""));
    assert_eq!(
        pulled(&config, &store, &Peer::answering(vec![retagged])),
        Sweep::Stopped(Stop::Source(
            "the answer says nothing changed of another entity tag than the one asked for"
                .to_owned()
        ))
    );
    let untagged = answered(FEED, (304, ""), &OLD, None);
    assert!(matches!(
        pulled(&config, &store, &Peer::answering(vec![untagged])),
        Sweep::Stopped(Stop::Source(_))
    ));
    let untagged_page = answered(FEED, (200, &listing(&[])), &OLD, None);
    pulled(&config, &store, &Peer::answering(vec![untagged_page]));
    assert_eq!(cursor(&store).unwrap().etag, None);
}

#[test]
fn an_older_or_equally_old_envelope_of_a_release_is_no_edit() {
    let store = healthy();
    pulling(&store, &[(1, noted("a/1", "the gate", 0))]);
    assert_eq!(
        pulling(&store, &[(2, noted("a/1", "the gate, fixed", 5))]),
        ["herald/upstream/a/1 edited"]
    );
    assert_eq!(
        pulling(&store, &[(3, noted("a/1", "the gate", 0))]),
        Vec::<String>::new()
    );
    assert_eq!(
        pulling(&store, &[(4, noted("a/1", "the gate, again", 5))]),
        Vec::<String>::new()
    );
    let identity = Identity::new("herald/upstream/a/1");
    assert_eq!(
        cursor(&store)
            .unwrap()
            .proof(&identity)
            .map(|proof| proof.created),
        Some(at(5))
    );
    assert_eq!(
        pulling(&store, &[(5, noted("a/1", "the gate, again", 6))]),
        ["herald/upstream/a/1 edited"]
    );
}

#[test]
fn a_relayed_envelope_edits_only_with_a_newer_proof_and_never_the_peer_s_own() {
    let store = healthy();
    pulling(
        &store,
        &[
            (2, noted("a/1", "the gate", 5)),
            (1, relayed("o/1", "the origin", 5)),
        ],
    );
    assert_eq!(
        pulling(&store, &[(3, relayed("o/1", "the origin, old", 1))]),
        Vec::<String>::new()
    );
    assert_eq!(
        pulling(&store, &[(4, relayed("a/1", "the gate, old", 1))]),
        ["herald/origin/a/1"]
    );
    assert_eq!(
        pulling(&store, &[(5, relayed("o/1", "the origin, fixed", 9))]),
        ["herald/origin/o/1 edited"]
    );
}

#[test]
fn a_release_a_tombstone_withdrew_enters_again_only_with_a_newer_proof() {
    let store = healthy();
    pulling(&store, &[(1, noted("a/1", "the gate", 0))]);
    let config = config("", FROM_PEER);
    let tombstoned = Peer::answering(vec![page(&[(2, tombstone("a/1", &OLD))], &OLD)]);
    assert_eq!(swept(&config, &store, &tombstoned).len(), 2);
    assert_eq!(
        pulling(&store, &[(3, noted("a/1", "the gate", 0))]),
        Vec::<String>::new()
    );
    assert_eq!(
        pulling(&store, &[(4, noted("a/1", "the gate, again", 62))]),
        ["herald/upstream/a/1"]
    );
}

#[test]
fn a_feed_listing_one_identity_twice_is_taken_once_by_its_newest_place() {
    let store = healthy();
    pulling(&store, &[(1, noted("a/0", "the start", 0))]);
    assert_eq!(
        pulling(
            &store,
            &[
                (3, noted("a/1", "the gate", 1)),
                (2, noted("a/1", "the gate, newer", 2)),
            ],
        ),
        ["herald/upstream/a/1"]
    );
    assert_eq!(
        cursor(&store)
            .unwrap()
            .proof(&Identity::new("herald/upstream/a/1"))
            .map(|proof| proof.created),
        Some(at(1))
    );
}

#[test]
fn a_peer_proving_its_own_envelopes_far_ahead_evicts_no_proof_a_replay_meets() {
    let store = healthy();
    assert_eq!(
        pulling(&store, &[(1, relayed("o/1", "the origin", 5))]),
        Vec::<String>::new()
    );
    let ahead = Peer::answering(vec![page(&[(2, noted("a/1", "the flood", 66))], &OLD)]);
    let config = config("", FROM_PEER);
    let sweeps = swept(&config, &store, &ahead);
    assert_eq!(sweeps[1], Sweep::Ahead("upstream".to_owned()));
    let kept = cursor(&store).unwrap();
    assert_eq!(kept.sequence, Some(2));
    assert_eq!(kept.proof(&Identity::new("herald/upstream/a/1")), None);
    assert_eq!(kept.ahead, ["upstream"]);
    assert_eq!(
        pollster::block_on(herald_flows::shared(&config, &store)).notes,
        [
            "sources.up: its last pull passed over an envelope of upstream whose proof was made too far ahead of now."
        ]
    );
    assert_eq!(
        pulling(&store, &[(2, relayed("o/1", "the origin, old", 1))]),
        Vec::<String>::new()
    );
    assert_eq!(
        pulling(&store, &[(3, relayed("o/1", "the origin, fixed", 9))]),
        ["herald/origin/o/1 edited"]
    );
}
