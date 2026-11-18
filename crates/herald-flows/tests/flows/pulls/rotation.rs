//! A peer whose rotation is signed: the key set it hands over to, proved
//! by a key trusted for it, taken and kept, and read again once due or
//! when an answer is signed by a key not trusted.

use herald_core::PeerKeys;
use herald_flows::{Stop, Sweep};
use herald_peers::keys::{KeySet, method};
use herald_peers::multibase::ed25519_public;
use herald_ports::Published;
use jiff::SignedDuration;

use super::*;
use crate::harness::{Fault, NOON, healthy};

const SIGNED: &str = "rotation = \"signed\"";

fn set(sequence: u64, keys: &[[u8; 32]]) -> KeySet {
    KeySet {
        sequence,
        refresh: 3600,
        keys: keys.to_vec(),
    }
}

fn kept(store: &Failing) -> Option<PeerKeys> {
    pollster::block_on(store.inner.peer_keys("upstream"))
        .unwrap()
        .map(|kept| kept.value)
}

fn keep(store: &Failing, keys: &[[u8; 32]], pinned: [u8; 32], read: jiff::Timestamp) {
    let held = PeerKeys {
        pinned: vec![ed25519_public(&pinned)],
        sequence: 2,
        keys: keys.iter().map(ed25519_public).collect(),
        read,
        refresh: 3600,
    };
    let written = pollster::block_on(store.inner.put_peer_keys("upstream", &held, None));
    assert!(matches!(written, Ok(herald_ports::Written::Written(_))));
}

#[test]
fn a_first_pull_takes_the_set_the_peer_hands_over_to_and_keeps_it() {
    let config = config(SIGNED, FROM_PEER);
    let store = healthy();
    let peer = Peer::answering(vec![
        discovered(&set(2, &[NEW, OLD]), Some(&OLD), &OLD),
        page(&[(1, release("a/1", &NEW))], &NEW),
    ]);
    assert!(matches!(
        pulled(&config, &store, &peer),
        Sweep::Polled { found: 1, .. }
    ));
    assert_eq!(peer.asked(), [DISCOVERY, FEED]);
    assert_eq!(
        kept(&store),
        Some(PeerKeys {
            pinned: vec![ed25519_public(&OLD)],
            sequence: 2,
            keys: vec![ed25519_public(&NEW), ed25519_public(&OLD)],
            read: NOON,
            refresh: 3600,
        })
    );
}

#[test]
fn a_kept_set_stands_until_due_or_until_an_answer_names_a_key_it_lacks() {
    let config = config(SIGNED, FROM_PEER);
    let store = healthy();
    keep(&store, &[OLD], OLD, NOON);
    let quiet = Peer::answering(vec![page(&[(1, release("a/1", &OLD))], &OLD)]);
    pulled(&config, &store, &quiet);
    assert_eq!(quiet.asked(), [FEED]);
    let renewed = Peer::answering(vec![
        page(&[(2, release("a/2", &NEW))], &NEW),
        discovered(&set(3, &[NEW, OLD]), Some(&OLD), &OLD),
        page(&[(2, release("a/2", &NEW))], &NEW),
    ]);
    assert_eq!(
        announced(&pulled(&config, &store, &renewed)),
        ["herald/upstream/a/2"]
    );
    assert_eq!(renewed.asked(), [FEED, DISCOVERY, FEED]);
    assert_eq!(kept(&store).unwrap().sequence, 3);
    let due = NOON.checked_sub(SignedDuration::from_hours(2)).unwrap();
    let late = healthy();
    keep(&late, &[OLD], OLD, due);
    let reread = Peer::answering(vec![
        discovered(&set(2, &[OLD]), Some(&OLD), &OLD),
        page(&[(1, release("a/1", &OLD))], &OLD),
    ]);
    pulled(&config, &late, &reread);
    assert_eq!(reread.asked(), [DISCOVERY, FEED]);
    assert_eq!(kept(&late).unwrap().read, NOON);
}

#[test]
fn a_set_kept_over_other_pinned_keys_stands_for_nothing() {
    let config = config(SIGNED, FROM_PEER);
    let store = healthy();
    keep(&store, &[NEW], ORIGIN, NOON);
    let peer = Peer::answering(vec![
        discovered(&set(1, &[OLD]), Some(&OLD), &OLD),
        page(&[(1, release("a/1", &OLD))], &OLD),
    ]);
    assert!(matches!(
        pulled(&config, &store, &peer),
        Sweep::Polled { found: 1, .. }
    ));
    assert_eq!(peer.asked(), [DISCOVERY, FEED]);
    assert_eq!(kept(&store).unwrap().pinned, [ed25519_public(&OLD)]);
}

#[test]
fn a_set_unproved_or_naming_a_revoked_key_is_not_taken() {
    let revoked = config(SIGNED, FROM_PEER);
    let mut revoked = revoked;
    revoked.federation.revoked.push(NEW);
    for (config, handed) in [
        (revoked, discovered(&set(2, &[NEW, OLD]), Some(&OLD), &OLD)),
        (
            config(SIGNED, FROM_PEER),
            discovered(&set(2, &[NEW, OLD]), Some(&NEW), &OLD),
        ),
        (
            config(SIGNED, FROM_PEER),
            discovered(&set(2, &[NEW, OLD]), None, &OLD),
        ),
        (
            config(SIGNED, FROM_PEER),
            Response {
                status: 500,
                headers: Vec::new(),
                body: Vec::new(),
            },
        ),
    ] {
        let store = healthy();
        let feed = page(&[(1, release("a/1", &NEW))], &NEW);
        let peer = Peer::answering(vec![handed.clone(), feed.clone(), handed, feed]);
        assert_eq!(
            pulled(&config, &store, &peer),
            Sweep::Stopped(Stop::Source(format!(
                "the feed's answer is signed by {}, a key not trusted for the peer",
                method(DOMAIN, &NEW)
            )))
        );
        assert_eq!(kept(&store).unwrap().keys, [ed25519_public(&OLD)]);
        assert_eq!(peer.asked(), [DISCOVERY, FEED, DISCOVERY, FEED]);
    }
}

#[test]
fn a_forged_proof_on_a_handed_over_set_is_refused() {
    let config = config(SIGNED, FROM_PEER);
    let store = healthy();
    let mut handed = discovered(&set(2, &[NEW, OLD]), Some(&OLD), &OLD);
    let forged: serde_json::Value = serde_json::from_slice(&handed.body).unwrap();
    let mut forged = forged;
    forged["keys"]["refresh"] = serde_json::json!(60);
    handed = signed(DISCOVERY, &forged.to_string(), &OLD);
    let peer = Peer::answering(vec![handed, page(&[(1, release("a/1", &OLD))], &OLD)]);
    assert!(matches!(
        pulled(&config, &store, &peer),
        Sweep::Polled { found: 1, .. }
    ));
    assert_eq!(kept(&store).unwrap().keys, [ed25519_public(&OLD)]);
}

#[test]
fn a_store_that_cannot_keep_a_peer_s_set_stops_the_pull() {
    let config = config(SIGNED, FROM_PEER);
    for (fault, stopped) in [
        (Fault::ReadsPeerKeys, true),
        (Fault::WritesPeerKeys, true),
        (Fault::ConflictsPeerKeys, false),
    ] {
        let store = Failing { fault, ..healthy() };
        let peer = Peer::answering(vec![
            discovered(&set(2, &[OLD]), Some(&OLD), &OLD),
            page(&[(1, release("a/1", &OLD))], &OLD),
        ]);
        let sweep = pulled(&config, &store, &peer);
        assert_eq!(
            matches!(sweep, Sweep::Stopped(Stop::Store(_))),
            stopped,
            "{sweep:?}"
        );
    }
}

#[test]
fn doctor_names_a_signed_peer_s_set_herald_toml_does_not_pin() {
    let config = config(SIGNED, FROM_PEER);
    let notes = |store: &Failing| pollster::block_on(herald_flows::handed_over(&config, store));
    let store = healthy();
    assert_eq!(notes(&store).notes, Vec::<String>::new());
    keep(&store, &[OLD], OLD, NOON);
    assert_eq!(notes(&store).notes, Vec::<String>::new());
    let drifted = healthy();
    keep(&drifted, &[NEW, OLD], OLD, NOON);
    assert_eq!(
        notes(&drifted).notes,
        [format!(
            "peers.upstream: the peer handed over to {}, {} under sequence 2, which herald.toml does not pin; herald peer add {PEER} --name upstream --update pins them.",
            ed25519_public(&NEW),
            ed25519_public(&OLD)
        )]
    );
    let other = healthy();
    keep(&other, &[NEW], ORIGIN, NOON);
    assert_eq!(notes(&other).notes, Vec::<String>::new());
    let unread = Failing {
        fault: Fault::ReadsPeerKeys,
        ..healthy()
    };
    let found = notes(&unread);
    let keys: Vec<&str> = found.gaps.iter().map(|gap| gap.key.as_str()).collect();
    assert_eq!(keys, ["peers.origin.rotation", "peers.upstream.rotation"]);
    assert_eq!(
        found.gaps[1].feature,
        "reading the key set the peer upstream handed over to"
    );
}

#[test]
fn a_set_is_read_again_no_later_than_the_longest_refresh_herald_allows() {
    let config = config(SIGNED, FROM_PEER);
    let store = healthy();
    let lasting = KeySet {
        refresh: 30 * 86_400,
        ..set(2, &[OLD])
    };
    let peer = Peer::answering(vec![
        discovered(&lasting, Some(&OLD), &OLD),
        page(&[(1, release("a/1", &OLD))], &OLD),
    ]);
    pulled(&config, &store, &peer);
    assert_eq!(kept(&store).unwrap().refresh, 7 * 86_400);
}

#[test]
fn another_peer_s_kept_set_proves_what_it_made_without_reading_it_again() {
    let config = config("", FROM_PEER);
    let store = healthy();
    let due = NOON.checked_sub(SignedDuration::from_hours(2)).unwrap();
    let origin = PeerKeys {
        pinned: vec![ed25519_public(&ORIGIN)],
        sequence: 2,
        keys: vec![ed25519_public(&NEW), ed25519_public(&ORIGIN)],
        read: due,
        refresh: 3600,
    };
    let written = pollster::block_on(store.inner.put_peer_keys("origin", &origin, None));
    assert!(matches!(written, Ok(herald_ports::Written::Written(_))));
    pulled(
        &config,
        &store,
        &Peer::answering(vec![page(&[(1, release("a/1", &OLD))], &OLD)]),
    );
    let relayed = proved(
        &envelope(
            "o/2",
            &["https://origin.example/.well-known/herald", DISCOVERY],
        ),
        "origin.example",
        &NEW,
    );
    let later = Peer::answering(vec![page(&[(2, relayed)], &OLD)]);
    assert_eq!(
        announced(&pulled(&config, &store, &later)),
        ["herald/origin/o/2"]
    );
    assert_eq!(later.asked(), [FEED]);
}

#[test]
fn a_key_revoked_after_its_set_was_kept_stops_standing_at_once() {
    let mut config = config(SIGNED, FROM_PEER);
    config.federation.revoked.push(NEW);
    let store = healthy();
    keep(&store, &[NEW, OLD], OLD, NOON);
    let feed = page(&[(1, release("a/1", &NEW))], &NEW);
    let handed = discovered(&set(3, &[NEW, OLD]), Some(&OLD), &OLD);
    let peer = Peer::answering(vec![handed.clone(), feed.clone(), handed, feed]);
    assert_eq!(
        pulled(&config, &store, &peer),
        Sweep::Stopped(Stop::Source(format!(
            "the feed's answer is signed by {}, a key not trusted for the peer",
            method(DOMAIN, &NEW)
        )))
    );
    assert_eq!(peer.asked(), [DISCOVERY, FEED, DISCOVERY, FEED]);
    assert_eq!(kept(&store).unwrap().keys, [ed25519_public(&OLD)]);
}

#[test]
fn a_revoked_key_proves_no_relayed_envelope_and_signs_no_pinned_feed() {
    let mut config = config("", FROM_PEER);
    config.federation.revoked.push(NEW);
    let store = healthy();
    let origin = PeerKeys {
        pinned: vec![ed25519_public(&ORIGIN)],
        sequence: 2,
        keys: vec![ed25519_public(&NEW), ed25519_public(&ORIGIN)],
        read: NOON,
        refresh: 3600,
    };
    let written = pollster::block_on(store.inner.put_peer_keys("origin", &origin, None));
    assert!(matches!(written, Ok(herald_ports::Written::Written(_))));
    let relayed = proved(
        &envelope(
            "o/1",
            &["https://origin.example/.well-known/herald", DISCOVERY],
        ),
        "origin.example",
        &NEW,
    );
    let sweep = pulled(
        &config,
        &store,
        &Peer::answering(vec![page(&[(1, relayed)], &OLD)]),
    );
    assert_eq!(
        sweep,
        Sweep::Stopped(Stop::Source(format!(
            "an envelope is proved by {}, a key not trusted for origin.example",
            method("origin.example", &NEW)
        )))
    );
    let source = format!("url = \"{FEED}\"\nkeys = [\"{}\"]", ed25519_public(&NEW));
    let mut pinned = super::config("", &source);
    pinned.federation.revoked.push(NEW);
    let refused = pulled(
        &pinned,
        &healthy(),
        &Peer::answering(vec![page(&[(1, release("a/1", &NEW))], &NEW)]),
    );
    assert_eq!(
        refused,
        Sweep::Stopped(Stop::Source(format!(
            "the feed's answer is signed by {}, a key not trusted for the peer",
            method(DOMAIN, &NEW)
        )))
    );
}

#[test]
fn a_pinned_key_revoked_since_stands_for_no_peer_on_any_path() {
    let stranger = |key: &[u8; 32]| {
        Sweep::Stopped(Stop::Source(format!(
            "the feed's answer is signed by {}, a key not trusted for the peer",
            method(DOMAIN, key)
        )))
    };
    let mut manual = config("", FROM_PEER);
    manual.federation.revoked.push(OLD);
    let feed = page(&[(1, release("a/1", &OLD))], &OLD);
    assert_eq!(
        pulled(&manual, &healthy(), &Peer::answering(vec![feed.clone()])),
        stranger(&OLD)
    );
    let mut signed = config(SIGNED, FROM_PEER);
    signed.federation.revoked.push(OLD);
    let handed = discovered(&set(2, &[OLD]), Some(&OLD), &OLD);
    let peer = Peer::answering(vec![handed.clone(), feed.clone(), handed, feed]);
    assert_eq!(pulled(&signed, &healthy(), &peer), stranger(&OLD));
    let mut relaying = config("", FROM_PEER);
    relaying.federation.revoked.push(ORIGIN);
    let relayed = proved(
        &envelope(
            "o/1",
            &["https://origin.example/.well-known/herald", DISCOVERY],
        ),
        "origin.example",
        &ORIGIN,
    );
    assert_eq!(
        pulled(
            &relaying,
            &healthy(),
            &Peer::answering(vec![page(&[(1, relayed)], &OLD)])
        ),
        Sweep::Stopped(Stop::Source(format!(
            "an envelope is proved by {}, a key not trusted for origin.example",
            method("origin.example", &ORIGIN)
        )))
    );
}
