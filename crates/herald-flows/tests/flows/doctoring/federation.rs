//! What doctor's federation section shows of a config that takes part
//! among other heralds: its keys, peers, grants, feeds, pushes and herald
//! sources (ADR-0058).

use herald_flows::{
    Federation, FeedShown, GrantShown, Here, OwnKey, Pulls, PushShown, RunShown, Shown,
    SourceShown, federation, secrets,
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
peers = {{ org = {{ actions = ["push"] }}, runs-ci = {{ actions = ["push"] }} }}
[apps.runs]
oidc = [{{ issuer = "https://issuer.example", claims = {{ repository_id = "42" }} }}]
peers = ["runs-ci"]
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
        [
            GrantShown {
                peer: "org".to_owned(),
                source: "inbox".to_owned(),
                actions: vec!["push"],
                until: None,
            },
            GrantShown {
                peer: "runs-ci".to_owned(),
                source: "inbox".to_owned(),
                actions: vec!["push"],
                until: None,
            },
        ]
    );
    assert_eq!(
        shown.runs,
        [RunShown {
            app: "runs".to_owned(),
            issuer: "https://issuer.example".to_owned(),
            audience: "https://herald.example".to_owned(),
            peers: vec!["runs-ci".to_owned()],
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
            id_token: false,
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

#[test]
fn a_host_that_does_not_announce_is_not_asked_for_the_pages_default_token() {
    let config = loaded(
        r#"version = 1
[store]
kind = "sqlite"
[federation]
domain = "herald.example"
keys = [{ secret = "HERALD_FED_KEY" }]
[federation.pages]
url = "https://herald.example"
"#,
    );
    assert!(config.secrets_read().contains("GITHUB_TOKEN"));
    let here = Here {
        online: true,
        ..Here::default()
    };
    let found = pollster::block_on(secrets(&config, &FakeSecrets::default(), &here));
    let asked: Vec<&str> = found.gaps.iter().map(|gap| gap.feature.as_str()).collect();
    assert_eq!(asked, ["reading the secret HERALD_FED_KEY"]);
}
