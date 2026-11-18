use std::num::NonZeroUsize;

use herald_core::{Cursor, Identity, Proof, Tombstone, Unheld};
use herald_peers::proof;
use herald_testing::fake_ed25519;
use jiff::Timestamp;
use serde_json::{Value, json};

use super::{
    DOMAIN, FEED, KEY, NOW, ORIGIN, ORIGIN_KEY, PEER, bare, identities, page, passed, pulled,
    source,
};
use crate::options::HeraldFile;

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(NOW + second).unwrap()
}

fn proved_at(envelope: &Value, created: Timestamp) -> Value {
    let unproved =
        proof::unproved(envelope, &herald_peers::keys::method(DOMAIN, &KEY), created).unwrap();
    let signature = fake_ed25519(&KEY, unproved.message());
    unproved.proved(&signature)
}

fn tombstone(identity: &str, created: Timestamp) -> Value {
    proved_at(
        &json!({ "type": "herald:Tombstone", "identity": identity, "path": [PEER] }),
        created,
    )
}

fn relayed_tombstone(identity: &str) -> Value {
    relayed(&json!({ "type": "herald:Tombstone", "identity": identity }))
}

fn relayed(envelope: &Value) -> Value {
    let mut envelope = envelope.clone();
    envelope["path"] = json!([ORIGIN, PEER]);
    let unproved = proof::unproved(
        &envelope,
        &herald_peers::keys::method("origin.example", &ORIGIN_KEY),
        at(1),
    )
    .unwrap();
    let signature = fake_ed25519(&ORIGIN_KEY, unproved.message());
    unproved.proved(&signature)
}

fn holding(proofs: &[(&str, i64)]) -> Cursor {
    Cursor {
        sequence: Some(1),
        proved: proofs
            .iter()
            .map(|(identity, second)| kept(identity, *second, false))
            .collect(),
        ..Cursor::default()
    }
}

fn kept(identity: &str, second: i64, tombstone: bool) -> Proof {
    Proof {
        identity: Identity::new(identity),
        created: at(second),
        maker: identity.split('/').nth(1).unwrap_or_default().to_owned(),
        tombstone: tombstone.then_some(Tombstone::Held),
        passed: false,
    }
}

fn taken(cursor: &Cursor, items: &[(u64, Value)]) -> herald_core::Polled {
    pulled(
        &source(HeraldFile::default()),
        vec![page(FEED, items, false)],
        Some(cursor),
    )
    .0
    .unwrap()
    .unwrap()
}

#[test]
fn an_envelope_is_taken_only_where_its_proof_is_newer_than_the_one_held() {
    let cursor = holding(&[("herald/upstream/a/1", 10), ("herald/upstream/a/2", 10)]);
    let read = taken(
        &cursor,
        &[
            (5, proved_at(&bare("a/1"), at(11))),
            (4, proved_at(&bare("a/2"), at(10))),
            (3, proved_at(&bare("a/3"), at(-100))),
        ],
    );
    assert_eq!(
        identities(&read),
        ["herald/upstream/a/1", "herald/upstream/a/3"]
    );
    assert_eq!(passed(&read), Vec::<&str>::new());
    assert_eq!(
        read.proved,
        [
            kept("herald/upstream/a/1", 11, false),
            kept("herald/upstream/a/3", -100, false),
        ]
    );
    let older = taken(&cursor, &[(5, proved_at(&bare("a/1"), at(9)))]);
    assert!(older.events.is_empty() && older.proved.is_empty());
}

#[test]
fn a_tombstone_is_taken_only_where_its_proof_is_newer_than_the_one_held() {
    let cursor = holding(&[("herald/upstream/a/1", 10)]);
    assert!(cursor.took(&Identity::new("herald/upstream/a/1")));
    let stale = taken(&cursor, &[(5, tombstone("a/1", at(10)))]);
    assert_eq!(stale.withdrawn, Vec::<Identity>::new());
    let fresh = taken(&cursor, &[(5, tombstone("a/1", at(11)))]);
    assert_eq!(fresh.withdrawn, [Identity::new("herald/upstream/a/1")]);
    assert_eq!(fresh.proved, [kept("herald/upstream/a/1", 11, true)]);
}

#[test]
fn an_identity_a_page_lists_twice_is_taken_by_its_newest_place_alone() {
    let read = taken(
        &holding(&[("herald/upstream/a/1", 0)]),
        &[
            (5, tombstone("a/1", at(1))),
            (4, proved_at(&bare("a/1"), at(2))),
        ],
    );
    assert_eq!(read.withdrawn, [Identity::new("herald/upstream/a/1")]);
    assert_eq!(identities(&read), Vec::<&str>::new());
    assert_eq!(read.proved, [kept("herald/upstream/a/1", 1, true)]);
}

#[test]
fn a_proof_made_further_ahead_than_the_skew_is_passed_over_and_the_next_taken() {
    let edge = taken(&holding(&[]), &[(5, proved_at(&bare("a/1"), at(300)))]);
    assert_eq!(edge.proved, [kept("herald/upstream/a/1", 300, false)]);
    assert_eq!(edge.ahead, Vec::<String>::new());
    let ahead = taken(
        &holding(&[]),
        &[
            (6, proved_at(&bare("a/1"), at(301))),
            (5, proved_at(&bare("a/2"), at(1))),
        ],
    );
    assert_eq!(ahead.ahead, ["upstream"]);
    assert_eq!(identities(&ahead), ["herald/upstream/a/2"]);
    assert_eq!(ahead.proved, [kept("herald/upstream/a/2", 1, false)]);
    assert_eq!(ahead.sequence, Some(6));
}

#[test]
fn an_instance_at_its_share_takes_no_new_identity_but_its_tombstones_still_withdraw() {
    let hop = source(HeraldFile {
        verify: crate::options::Verify::Hop,
        peer_share: NonZeroUsize::new(2),
        ..HeraldFile::default()
    });
    let cursor = Cursor {
        etag: Some("\"seen\"".to_owned()),
        proved: vec![
            kept("herald/origin/a/1", 0, false),
            kept("herald/origin/a/2", 0, false),
            kept("herald/origin/h/1", 0, true),
            kept("herald/origin/h/2", 0, true),
            kept("herald/upstream/b/1", 0, false),
        ],
        ..holding(&[])
    };
    let read = pulled(
        &hop,
        vec![page(
            FEED,
            &[
                (7, relayed_tombstone("a/1")),
                (6, relayed(&bare("a/3"))),
                (5, tombstone("b/1", at(1))),
                (4, proved_at(&bare("b/2"), at(1))),
                (3, relayed(&bare("a/2"))),
            ],
            false,
        )],
        Some(&cursor),
    )
    .0
    .unwrap()
    .unwrap();
    assert_eq!(read.refused, ["origin"]);
    assert_eq!(
        read.withdrawn,
        [
            Identity::new("herald/origin/a/1"),
            Identity::new("herald/upstream/b/1")
        ]
    );
    assert_eq!(
        identities(&read),
        ["herald/upstream/b/2", "herald/origin/a/2"]
    );
    assert_eq!(read.sequence, Some(5));
    assert_eq!(read.etag, None);
    assert_eq!(read.proved[0].tombstone, Some(Tombstone::Held));
    assert_eq!(read.each, Some(200));
    assert_eq!(taken(&cursor, &[]).each, Some(400));
}

#[test]
fn no_tombstone_of_an_identity_taken_is_forgotten_however_many_the_source_holds() {
    let few = source(HeraldFile {
        tombstones: NonZeroUsize::new(2),
        ..HeraldFile::default()
    });
    let cursor = Cursor {
        proved: vec![
            kept("herald/upstream/a/1", 0, true),
            kept("herald/upstream/a/2", 0, true),
            kept("herald/upstream/a/3", 0, true),
            kept("herald/upstream/a/4", 0, false),
        ],
        ..holding(&[])
    };
    let read = taken(&cursor, &[(6, tombstone("a/4", at(1)))]);
    assert_eq!(read.withdrawn, [Identity::new("herald/upstream/a/4")]);
    let full = pulled(
        &few,
        vec![page(FEED, &[(6, tombstone("a/4", at(1)))], false)],
        Some(&cursor),
    )
    .0
    .unwrap()
    .unwrap();
    assert_eq!(full.refused, Vec::<String>::new());
    assert_eq!(full.unheld, Unheld { most: 2, each: 2 });
    let advanced = cursor.advanced(&full, 0);
    for gone in ["a/1", "a/2", "a/3", "a/4"] {
        let identity = Identity::new(format!("herald/upstream/{gone}"));
        assert!(advanced.took(&identity), "{gone}");
        assert!(!advanced.newer(&identity, at(0)), "{gone}");
    }
}

#[test]
fn a_tombstone_of_an_identity_never_taken_is_kept_only_as_one_others_push_out() {
    let budget = source(HeraldFile {
        tombstones: NonZeroUsize::new(4),
        peer_tombstones: NonZeroUsize::new(3),
        ..HeraldFile::default()
    });
    let cursor = Cursor {
        sequence: Some(1),
        proved: vec![kept("herald/upstream/a/1", 0, true)],
        ..Cursor::default()
    };
    let flood: Vec<(u64, Value)> = (2..12)
        .rev()
        .map(|sequence| (sequence, tombstone(&format!("x/{sequence}"), at(1))))
        .collect();
    let read = pulled(&budget, vec![page(FEED, &flood, false)], Some(&cursor))
        .0
        .unwrap()
        .unwrap();
    assert_eq!(read.withdrawn, Vec::<Identity>::new());
    assert_eq!(read.unheld, Unheld { most: 4, each: 3 });
    assert_eq!(read.proved.len(), 10);
    assert!(
        read.proved
            .iter()
            .all(|proof| proof.tombstone == Some(Tombstone::Unheld))
    );
    let advanced = cursor.advanced(&read, 0);
    assert_eq!(advanced.proved.len(), 4);
    assert!(!advanced.took(&Identity::new("herald/upstream/x/11")));
    let again = pulled(
        &budget,
        vec![page(FEED, &[(12, tombstone("x/11", at(2)))], false)],
        Some(&advanced),
    )
    .0
    .unwrap()
    .unwrap();
    assert_eq!(again.proved[0].tombstone, Some(Tombstone::Unheld));
    assert_eq!(again.withdrawn, Vec::<Identity>::new());
}

#[test]
fn a_proof_made_at_no_time_is_refused() {
    let mut envelope = proved_at(&bare("a/1"), at(0));
    envelope["proof"]["created"] = json!("soon");
    let shown = proof::shown(&envelope).unwrap();
    envelope["proof"]["proofValue"] = json!(herald_peers::multibase::encode(&fake_ed25519(
        &KEY,
        &shown.message
    )));
    assert_eq!(
        pulled(
            &source(HeraldFile::default()),
            vec![page(FEED, &[(5, envelope)], false)],
            None
        )
        .0,
        Err(herald_kinds::Unpulled::Unreadable(
            "an envelope's proof was made at soon, which is no time".to_owned()
        ))
    );
}

#[test]
fn a_tombstone_of_an_event_passed_over_or_only_seen_is_kept_only_as_one_others_push_out() {
    let mut unknown = bare("a/1");
    unknown["kind"] = json!("Not A Kind");
    let cursor = Cursor {
        seen: vec![Identity::new("herald/upstream/a/2")],
        ..holding(&[])
    };
    let read = taken(&cursor, &[(5, proved_at(&unknown, at(1)))]);
    assert_eq!(passed(&read), ["herald/upstream/a/1"]);
    assert_eq!(
        read.proved,
        [Proof {
            passed: true,
            ..kept("herald/upstream/a/1", 1, false)
        }]
    );
    let advanced = cursor.advanced(&read, 10);
    let unheld = Some(Tombstone::Unheld);
    for identity in ["a/1", "a/2"] {
        let gone = taken(&advanced, &[(6, tombstone(identity, at(2)))]);
        assert_eq!(gone.withdrawn, Vec::<Identity>::new(), "{identity}");
        assert_eq!(gone.proved[0].tombstone, unheld, "{identity}");
    }
}

#[test]
fn a_share_counts_only_new_identities_an_event_brings_and_names_each_instance_once() {
    let one = source(HeraldFile {
        peer_share: NonZeroUsize::new(1),
        ..HeraldFile::default()
    });
    let cursor = Cursor {
        proved: vec![
            kept("herald/upstream/a/1", 0, false),
            kept("herald/upstream/h/1", 0, true),
            Proof {
                tombstone: Some(Tombstone::Unheld),
                ..kept("herald/upstream/x/1", 0, false)
            },
        ],
        ..holding(&[])
    };
    let read = pulled(
        &one,
        vec![page(
            FEED,
            &[
                (9, tombstone("x/2", at(1))),
                (8, proved_at(&bare("x/1"), at(1))),
                (7, proved_at(&bare("a/2"), at(1))),
                (6, proved_at(&bare("a/3"), at(400))),
                (5, proved_at(&bare("a/4"), at(400))),
            ],
            false,
        )],
        Some(&cursor),
    )
    .0
    .unwrap()
    .unwrap();
    assert_eq!(read.refused, ["upstream"]);
    assert_eq!(read.ahead, ["upstream"]);
    assert_eq!(identities(&read), Vec::<&str>::new());
    assert_eq!(read.sequence, Some(6));
    assert_eq!(
        read.proved,
        [Proof {
            tombstone: Some(Tombstone::Unheld),
            ..kept("herald/upstream/x/2", 1, false)
        }]
    );
}

#[test]
fn an_instance_taking_its_whole_budget_never_evicts_another_s_proof() {
    let hop = source(HeraldFile {
        verify: crate::options::Verify::Hop,
        pages: NonZeroUsize::new(1),
        ..HeraldFile::default()
    });
    let cursor = Cursor {
        proved: vec![kept("herald/upstream/b/1", 10, false)],
        ..holding(&[])
    };
    let flood: Vec<(u64, Value)> = (2..43)
        .rev()
        .map(|sequence| (sequence, relayed(&bare(&format!("o/{sequence}")))))
        .collect();
    let read = pulled(&hop, vec![page(FEED, &flood, false)], Some(&cursor))
        .0
        .unwrap()
        .unwrap();
    assert_eq!(read.each, Some(40));
    let advanced = cursor.advanced(&read, 80);
    assert_eq!(
        advanced.proof(&Identity::new("herald/upstream/b/1")),
        Some(&kept("herald/upstream/b/1", 10, false))
    );
    let replay = pulled(
        &hop,
        vec![page(FEED, &[(43, proved_at(&bare("b/1"), at(5)))], false)],
        Some(&advanced),
    )
    .0
    .unwrap()
    .unwrap();
    assert_eq!(identities(&replay), Vec::<&str>::new());
}

#[test]
fn a_tombstone_taken_in_a_pull_counts_once_toward_the_share_of_the_envelopes_after_it() {
    let share = |share: usize| {
        source(HeraldFile {
            peer_share: NonZeroUsize::new(share),
            ..HeraldFile::default()
        })
    };
    let refused = |source: &super::Pulled, cursor: &Cursor| {
        pulled(
            source,
            vec![page(
                FEED,
                &[
                    (6, tombstone("a/1", at(1))),
                    (5, proved_at(&bare("a/9"), at(1))),
                ],
                false,
            )],
            Some(cursor),
        )
        .0
        .unwrap()
        .unwrap()
        .refused
    };
    let live = Cursor {
        proved: vec![kept("herald/upstream/a/1", 0, false)],
        ..holding(&[])
    };
    assert_eq!(refused(&share(1), &live), ["upstream"]);
    let held = Cursor {
        proved: vec![kept("herald/upstream/a/1", 0, true)],
        ..holding(&[])
    };
    assert_eq!(refused(&share(2), &held), Vec::<String>::new());
}

#[test]
fn an_instance_s_held_tombstones_never_shrink_another_s_room_for_the_rest() {
    let hop = source(HeraldFile {
        verify: crate::options::Verify::Hop,
        tombstones: NonZeroUsize::new(2),
        peer_tombstones: NonZeroUsize::new(2),
        ..HeraldFile::default()
    });
    let cursor = Cursor {
        proved: vec![
            kept("herald/origin/h/1", 0, true),
            kept("herald/origin/h/2", 0, true),
            kept("herald/origin/h/3", 0, true),
        ],
        ..holding(&[])
    };
    let read = pulled(
        &hop,
        vec![page(
            FEED,
            &[(6, tombstone("x/1", at(1))), (5, tombstone("x/2", at(1)))],
            false,
        )],
        Some(&cursor),
    )
    .0
    .unwrap()
    .unwrap();
    assert_eq!(read.unheld, Unheld { most: 2, each: 2 });
    let advanced = cursor.advanced(&read, 80);
    for stranger in ["x/1", "x/2"] {
        let identity = Identity::new(format!("herald/upstream/{stranger}"));
        assert!(!advanced.newer(&identity, at(1)), "{stranger}");
    }
    assert_eq!(advanced.held().get("origin"), Some(&3));
}
