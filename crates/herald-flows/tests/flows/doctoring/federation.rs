//! What doctor's federation section shows of a config that takes part
//! among other heralds: its keys, peers, grants, feeds, pushes and herald
//! sources (ADR-0058).

use herald_flows::{
    Federation, FeedShown, GrantShown, OwnKey, Pulls, PushShown, Shown, SourceShown, federation,
};
use herald_peers::keys::fingerprint;
use herald_peers::multibase::ed25519_public;
use herald_testing::{FakeSecrets, FakeSigner};

use super::{actions, loaded};

const OWN: [u8; 32] = [b'k'; 32];
const PINNED: [u8; 32] = [5; 32];

/// The section of a config with a key set here and one not, a private
/// peer granted a push, a feed, a push and two herald sources.
fn shown() -> Federation {
    let config = loaded(&format!(
        r#"version = 1
[store]
kind = "sqlite"
[federation]
domain = "herald.example"
keys = [{{ secret = "HERALD_FED_KEY" }}, {{ secret = "HERALD_FED_UNSET" }}]
[peers.org]
url = "https://herald.peer.example"
keys = ["{pinned}"]
private = true
grants = [{{ resource = "sources.inbox", actions = ["push"] }}]
[destinations.feed]
kind = "herald"
[destinations.mirror]
kind = "herald-push"
peer = "org"
inbox = "up"
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
[sources.inbox]
kind = "herald"
peers = {{ org = {{ actions = ["push"] }} }}
[sources.pulled]
kind = "herald"
peer = "org"
feed = "releases"
[[routes]]
id = "fed"
sources = ["gate"]
destination = "feed"
[[routes]]
id = "pushed"
sources = ["gate"]
destination = "mirror"
"#,
        pinned = ed25519_public(&PINNED),
    ));
    let secrets = FakeSecrets::default().with("HERALD_FED_KEY", str::from_utf8(&OWN).unwrap());
    pollster::block_on(federation(
        &config,
        &actions(),
        (&secrets, &FakeSigner),
        &[],
    ))
    .unwrap()
}

#[test]
fn doctor_shows_the_instance_s_keys_peers_and_grants() {
    let shown = shown();
    assert_eq!(
        shown.keys,
        [
            OwnKey {
                secret: "HERALD_FED_KEY".to_owned(),
                shown: Some(Shown::of(&OWN)),
            },
            OwnKey {
                secret: "HERALD_FED_UNSET".to_owned(),
                shown: None,
            },
        ]
    );
    assert_eq!(
        Shown::of(&OWN).to_string(),
        format!("{} {}", ed25519_public(&OWN), fingerprint(&OWN))
    );
    let peers: Vec<(&str, &str, &[Shown], bool)> = shown
        .peers
        .iter()
        .map(|peer| {
            (
                peer.name.as_str(),
                peer.url.as_str(),
                peer.keys.as_slice(),
                peer.private,
            )
        })
        .collect();
    assert_eq!(
        peers,
        [(
            "org",
            "https://herald.peer.example",
            &[Shown::of(&PINNED)][..],
            true
        )]
    );
    assert_eq!(
        shown.grants,
        [GrantShown {
            peer: "org".to_owned(),
            source: "inbox".to_owned(),
            actions: vec!["push"],
            until: None,
        }]
    );
}

#[test]
fn doctor_shows_the_instance_s_feeds_pushes_and_herald_sources() {
    let shown = shown();
    assert_eq!(
        shown.feeds,
        [FeedShown {
            name: "feed".to_owned(),
            url: "https://herald.example/herald/v1/feeds/feed".to_owned(),
            served: false,
        }]
    );
    assert_eq!(
        shown.pushes,
        [PushShown {
            name: "mirror".to_owned(),
            peer: "org".to_owned(),
            inbox: "https://herald.peer.example/herald/v1/inbox/up".to_owned(),
            private: true,
        }]
    );
    assert_eq!(
        shown.sources,
        [
            SourceShown {
                name: "inbox".to_owned(),
                pulls: None,
                pushed: true,
                stale_after: None,
            },
            SourceShown {
                name: "pulled".to_owned(),
                pulls: Some(Pulls::Peer("org".to_owned())),
                pushed: false,
                stale_after: None,
            },
        ]
    );
    assert_eq!(shown.routes, [0, 1]);
}
