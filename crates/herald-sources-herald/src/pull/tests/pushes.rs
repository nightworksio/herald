use std::collections::{BTreeMap, BTreeSet};

use herald_core::{Action, Cursor, Grant, Identity, KindSpecs, Polled, Proof, Tombstone};
use herald_kinds::Pulling;
use herald_peers::proof;
use herald_testing::{FakeVerifier, ScriptedHttp, fake_ed25519};
use jiff::Timestamp;
use serde_json::{Value, json};

use super::{DOMAIN, KEY, NOW, PEER, URL, bare, envelope, trust};
use crate::HeraldKind;
use crate::options::{HeraldFile, HeraldOptions};
use crate::push::Unreceived;
use herald_kinds::SourceKind as _;

fn grant(namespaces: &[&str]) -> Grant {
    Grant {
        actions: BTreeSet::from([Action::Push]),
        until: None,
        namespaces: namespaces
            .iter()
            .map(|namespace| (*namespace).to_owned())
            .collect(),
    }
}

fn inbox(share: usize) -> HeraldOptions {
    HeraldKind::checked(HeraldFile {
        peers: BTreeMap::from([("upstream".to_owned(), grant(&[]))]),
        peer_share: std::num::NonZeroUsize::new(share),
        ..HeraldFile::default()
    })
    .unwrap()
}

fn received(
    options: &HeraldOptions,
    cursor: Option<&Cursor>,
    granted: &Grant,
    envelope: &Value,
) -> Result<Polled, Unreceived> {
    received_again(options, cursor, granted, (envelope, false))
}

fn received_again(
    options: &HeraldOptions,
    cursor: Option<&Cursor>,
    granted: &Grant,
    (envelope, again): (&Value, bool),
) -> Result<Polled, Unreceived> {
    let http = ScriptedHttp::new(Vec::new());
    let specs = KindSpecs::default();
    let pulling = Pulling {
        http: &http,
        verifier: &FakeVerifier,
        specs: &specs,
        now: Timestamp::from_second(NOW).unwrap(),
    };
    pollster::block_on(options.received(
        (pulling, &trust(), cursor),
        ("upstream", granted),
        (envelope, again),
    ))
}

fn proved_at(envelope: &Value, second: i64) -> Value {
    let unproved = proof::unproved(
        envelope,
        &herald_peers::keys::method(DOMAIN, &KEY),
        Timestamp::from_second(NOW + second).unwrap(),
    )
    .unwrap();
    let signature = fake_ed25519(&KEY, unproved.message());
    unproved.proved(&signature)
}

#[test]
fn a_pushed_envelope_is_taken_under_the_pusher_s_name_unless_its_grant_gives_it() {
    let options = inbox(10);
    let taken = received(&options, None, &grant(&[]), &envelope("github.com/1/v1")).unwrap();
    assert_eq!(
        taken.events[0].identity().as_str(),
        "herald/upstream/github.com/1/v1"
    );
    assert_eq!(taken.proved[0].maker, "upstream");
    assert!(taken.relisted);
    assert_eq!(taken.each, Some(400));
    assert_eq!(taken.unheld.most, 1000);
    let given = received(
        &options,
        None,
        &grant(&["github.com/1"]),
        &envelope("github.com/1/v1"),
    )
    .unwrap();
    assert_eq!(given.events[0].identity().as_str(), "github.com/1/v1");
    assert_eq!(given.events[0].origin().unwrap().host, DOMAIN);
}

#[test]
fn a_pushed_envelope_no_newer_than_the_cursor_s_proof_adds_nothing() {
    let options = inbox(10);
    let first = received(&options, None, &grant(&[]), &envelope("github.com/1/v1")).unwrap();
    let cursor = Cursor::default().received(&first, 400);
    let again = received(
        &options,
        Some(&cursor),
        &grant(&[]),
        &envelope("github.com/1/v1"),
    )
    .unwrap();
    assert!(again.events.is_empty() && again.proved.is_empty() && again.passed.is_empty());
}

#[test]
fn a_pushed_envelope_another_made_ahead_or_unproved_is_refused() {
    let options = inbox(10);
    let mut relayed = bare("github.com/1/v1");
    relayed["path"] = json!(["https://elsewhere.example/.well-known/herald", PEER]);
    assert_eq!(
        received(&options, None, &grant(&[]), &proved_at(&relayed, 0)),
        Err(Unreceived::Unmade)
    );
    assert_eq!(
        received(
            &options,
            None,
            &grant(&[]),
            &proved_at(&bare("github.com/1/v1"), 301)
        ),
        Err(Unreceived::Ahead)
    );
    let mut forged = envelope("github.com/1/v1");
    forged["title"] = json!("forged");
    let unproved = received(&options, None, &grant(&[]), &forged).unwrap_err();
    assert_eq!(
        unproved,
        Unreceived::Unreadable("an envelope's proof does not hold".to_owned())
    );
    assert!(matches!(
        received(&options, None, &grant(&[]), &json!({})),
        Err(Unreceived::Unreadable(_))
    ));
    assert_eq!(
        [
            Unreceived::Unmade,
            Unreceived::Ahead,
            Unreceived::Spent,
            Unreceived::Unreadable("why".to_owned()),
        ]
        .map(|refused| refused.to_string()),
        [
            "the envelope came by a path the source does not take",
            "the envelope's proof was made too far ahead of now",
            "the instance that made the envelope holds as many tombstones as the source keeps",
            "why",
        ]
    );
    assert_eq!(URL, "https://peer.example");
}

#[test]
fn a_tombstone_withdraws_an_identity_taken_and_a_spent_instance_takes_no_new_one() {
    let options = inbox(1);
    let first = received(&options, None, &grant(&[]), &envelope("github.com/1/v1")).unwrap();
    let cursor = Cursor::default().received(&first, 400);
    let tombstone = proved_at(
        &json!({ "type": "herald:Tombstone", "identity": "github.com/1/v1", "path": [PEER] }),
        1,
    );
    let withdrawn = received(&options, Some(&cursor), &grant(&[]), &tombstone).unwrap();
    assert_eq!(
        withdrawn.withdrawn,
        vec![Identity::new("herald/upstream/github.com/1/v1")]
    );
    let held = cursor.received(&withdrawn, 400);
    assert!(held.proved.iter().any(Proof::held));
    assert_eq!(
        received(
            &options,
            Some(&held),
            &grant(&[]),
            &envelope("github.com/1/v2")
        ),
        Err(Unreceived::Spent)
    );
    assert_eq!(held.proved[0].tombstone, Some(Tombstone::Held));
}

#[test]
fn a_push_worked_again_takes_the_very_proof_its_cursor_holds_but_no_older_one() {
    let options = inbox(10);
    let first = received(&options, None, &grant(&[]), &envelope("github.com/1/v1")).unwrap();
    let cursor = Cursor::default().received(&first, 400);
    let again = received_again(
        &options,
        Some(&cursor),
        &grant(&[]),
        (&envelope("github.com/1/v1"), true),
    )
    .unwrap();
    assert_eq!(again.events.len(), 1);
    let newer = received(
        &options,
        None,
        &grant(&[]),
        &proved_at(&bare("github.com/1/v1"), 1),
    )
    .unwrap();
    let ahead = Cursor::default().received(&newer, 400);
    let older = received_again(
        &options,
        Some(&ahead),
        &grant(&[]),
        (&envelope("github.com/1/v1"), true),
    )
    .unwrap();
    assert!(older.events.is_empty() && older.proved.is_empty());
}
