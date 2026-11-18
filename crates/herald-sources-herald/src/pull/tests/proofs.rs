use std::collections::BTreeMap;

use herald_core::{Cursor, Identity};
use herald_peers::proof;
use herald_testing::fake_ed25519;
use jiff::Timestamp;
use serde_json::{Value, json};

use super::{DOMAIN, FEED, KEY, NOW, PEER, bare, identities, page, passed, pulled, source};
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

fn holding(proofs: &[(&str, i64)]) -> Cursor {
    Cursor {
        sequence: Some(1),
        proved: proofs
            .iter()
            .map(|(identity, second)| (Identity::new(*identity), at(*second)))
            .collect(),
        ..Cursor::default()
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
        BTreeMap::from([
            (Identity::new("herald/upstream/a/1"), at(11)),
            (Identity::new("herald/upstream/a/3"), at(-100)),
        ])
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
}

#[test]
fn an_identity_a_page_lists_twice_is_taken_by_its_newest_place_alone() {
    let seen = Cursor {
        seen: vec![Identity::new("herald/upstream/a/1")],
        ..holding(&[])
    };
    let read = taken(
        &seen,
        &[
            (5, tombstone("a/1", at(1))),
            (4, proved_at(&bare("a/1"), at(2))),
        ],
    );
    assert_eq!(read.withdrawn, [Identity::new("herald/upstream/a/1")]);
    assert_eq!(identities(&read), Vec::<&str>::new());
    assert_eq!(read.proved[&Identity::new("herald/upstream/a/1")], at(1));
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
