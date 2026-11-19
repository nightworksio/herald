//! `[peers.*]`: the heralds this one trusts, each by its pinned keys, its
//! address and its bounds.

use std::num::NonZeroU32;

use herald_config::{Config, Peer, Rotation, load};
use herald_core::{Per, Rate};
use herald_peers::multibase::ed25519_public;

fn nothing(_: &str) -> Result<String, String> {
    Err("no file".to_owned())
}

fn loaded(peers: &str) -> Result<Config, String> {
    let text = format!(
        "version = 1\n[store]\nkind = \"git-branch\"\nkey = {{ secret = \"HERALD_STORE_KEY\" }}\n{peers}"
    );
    load("herald.toml", &text, &nothing, &crate::common::kinds())
        .map_err(|errors| errors.to_string())
}

fn key(byte: u8) -> String {
    ed25519_public(&[byte; 32])
}

fn peer(name: &str, rest: &str) -> String {
    format!(
        "[peers.{name}]\nurl = \"https://herald.example\"\nkeys = [\"{}\"]\n{rest}\n",
        key(1)
    )
}

#[test]
fn a_peer_is_read_with_its_keys_and_the_bounds_herald_ships() {
    let text = format!(
        "[peers.org]\nurl = \"https://Herald.Example/api/\"\nkeys = [\"{}\", \"{}\"]\n",
        key(2),
        key(3)
    );
    let config = loaded(&text).unwrap();
    assert_eq!(
        config.peers["org"],
        Peer {
            url: "https://herald.example/api".to_owned(),
            domain: "herald.example".to_owned(),
            keys: vec![[2; 32], [3; 32]],
            rotation: Rotation::Manual,
            rate: Peer::RATE,
            most: Peer::MOST,
            hops: Peer::HOPS,
            private: false,
            grants: std::collections::BTreeMap::new(),
        }
    );
    assert_eq!(
        config.peers["org"].discovery(),
        "https://herald.example/api/.well-known/herald"
    );
    assert_eq!(Peer::RATE.to_string(), "120/m");
    assert_eq!(Peer::RATE_ALLOWED.to_string(), "480/m");
    assert_eq!(Peer::MOST, 2 << 20);
    assert_eq!(Peer::MOST_ALLOWED, 8 << 20);
    assert_eq!((Peer::HOPS, Peer::HOPS_ALLOWED), (2, 8));
    assert!(loaded("").unwrap().peers.is_empty());
}

#[test]
fn every_setting_a_peer_names_is_its_own() {
    let config = loaded(&peer(
        "org",
        "rotation = \"signed\"\nrate = \"8/s\"\nmost = \"8MiB\"\nhops = 8\nprivate = true",
    ))
    .unwrap();
    let read = &config.peers["org"];
    assert_eq!(read.rotation, Rotation::Signed);
    assert_eq!(
        read.rate,
        Rate::new(NonZeroU32::new(8).unwrap(), Per::Second)
    );
    assert_eq!((read.most, read.hops, read.private), (8 << 20, 8, true));
    assert_eq!(
        (Rotation::Manual.name(), Rotation::Signed.name()),
        ("manual", "signed")
    );
    let lowest = loaded(&peer("org", "hops = 1\nmost = \"1KiB\"")).unwrap();
    assert_eq!(
        (lowest.peers["org"].hops, lowest.peers["org"].most),
        (1, 1024)
    );
}

#[test]
fn a_bound_past_its_most_is_refused() {
    let refused = |rest: &str| loaded(&peer("org", rest)).unwrap_err();
    assert_eq!(
        refused("rate = \"9/s\""),
        "herald.toml:5:1 peers.org.rate: rate is at most \"480/m\"; lower it"
    );
    assert_eq!(
        refused("most = \"8193KiB\""),
        "herald.toml:5:1 peers.org.most: most is at most \"8MiB\"; lower it"
    );
    for hops in ["hops = 9", "hops = 0"] {
        assert_eq!(
            refused(hops),
            "herald.toml:5:1 peers.org.hops: hops is from 1 to 8",
            "{hops}"
        );
    }
}

#[test]
fn a_peer_is_reached_at_an_https_domain_alone() {
    for url in [
        "http://herald.example",
        "https://herald.example:8443",
        "https://127.0.0.1",
        "https://localhost",
        "https://user@herald.example",
        "https://herald.example/?page=1",
        "https://herald.example/#top",
        "herald.example",
    ] {
        let text = format!("[peers.org]\nurl = \"{url}\"\nkeys = [\"{}\"]\n", key(1));
        let error = loaded(&text).unwrap_err();
        assert!(
            error.contains("peers.org.url: \"") && error.contains("is not a peer's address"),
            "{url}: {error}"
        );
    }
}

#[test]
fn a_peer_pins_each_of_its_keys_once_and_at_least_one() {
    let pinned = |keys: &str| {
        loaded(&format!(
            "[peers.org]\nurl = \"https://herald.example\"\nkeys = [{keys}]\n"
        ))
    };
    assert!(
        pinned("")
            .unwrap_err()
            .contains("peers.org.keys: pin at least one of the peer's keys")
    );
    let twice = format!("\"{0}\", \"{0}\"", key(4));
    assert!(
        pinned(&twice)
            .unwrap_err()
            .contains(&format!("\"{}\" is pinned twice", key(4)))
    );
    assert!(
        pinned("\"z6MkNotAKey\"")
            .unwrap_err()
            .contains("\"z6MkNotAKey\" is not an Ed25519 Multikey")
    );
}

#[test]
fn a_key_pinned_for_two_peers_is_refused() {
    let text = format!("{}{}", peer("first", ""), peer("second", ""));
    assert_eq!(
        loaded(&text).unwrap_err(),
        "herald.toml:9:1 peers.second.keys: a key is pinned for both first and second; a key stands for one peer, so pin it for one"
    );
}

#[test]
fn a_peer_pinning_a_revoked_key_is_refused() {
    let revoked = |keys: &str| {
        loaded(&format!(
            "[federation]\nrevoked = [{keys}]\n{}",
            peer("org", "")
        ))
    };
    assert_eq!(
        revoked(&format!("\"{}\"", key(1))).unwrap_err(),
        format!(
            "herald.toml:7:1 peers.org.keys: the key {} is revoked in [federation] revoked; pin a key the peer has not lost",
            key(1)
        )
    );
    let config = revoked(&format!("\"{}\"", key(9))).unwrap();
    assert_eq!(config.federation.revoked, vec![[9; 32]]);
    assert_eq!(
        revoked("\"z6MkLost\"").unwrap_err(),
        "herald.toml:5:1 federation.revoked: \"z6MkLost\" is not an Ed25519 Multikey; give each revoked key as `herald peer revoke` writes it, such as \"z6Mk…\""
    );
}

#[test]
fn a_peer_s_name_is_a_name_its_identities_can_carry() {
    assert!(
        loaded(&peer("\"Org.Example\"", ""))
            .unwrap_err()
            .contains("\"Org.Example\" names the peer in its events' identities")
    );
    assert!(
        loaded("[peers.org]\nurl = 1\n")
            .unwrap_err()
            .contains("peers.org.url")
    );
    let text = "version = 1\npeers = 1\n[store]\nkind = \"git-branch\"\nkey = { secret = \"K\" }\n";
    let error = load("herald.toml", text, &nothing, &crate::common::kinds())
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "herald.toml:2:9 peers: peers is a table; write [peers]"
    );
}
