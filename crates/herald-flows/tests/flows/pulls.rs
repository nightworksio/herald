//! Another herald's feed pulled: through the keys that stand for its peer,
//! quiet on the first pull, each event it has not seen announced after,
//! and a key set the peer hands over to taken where its rotation is
//! signed.

use herald_config::{Config, Uses, load};
use herald_core::{Cursor, SourceName};
use herald_flows::{Connected, Outcome, Ports, Remembered, Stop, Sweep, Swept, poll, prepare};
use herald_peers::answer::{CONTENT_TYPE, ETAG, demanded};
use herald_peers::digest::{CONTENT_DIGEST, content_digest};
use herald_peers::keys::{KeySet, method};
use herald_peers::message::{Component, Covered, Message};
use herald_peers::multibase::ed25519_public;
use herald_peers::proof;
use herald_ports::{Cursors, Response};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner, FakeVerifier, fake_ed25519};
use serde_json::{Value, json};

use crate::harness::{Failing, NOON, Script, healthy};

mod replays;
mod rotation;
pub(crate) mod tombstones;

pub(crate) const PEER: &str = "https://peer.example";
pub(crate) const DOMAIN: &str = "peer.example";
pub(crate) const FEED: &str = "https://peer.example/herald/v1/feeds/releases";
pub(crate) const DISCOVERY: &str = "https://peer.example/.well-known/herald";
pub(crate) const OLD: [u8; 32] = [7; 32];
pub(crate) const NEW: [u8; 32] = [8; 32];
pub(crate) const ORIGIN: [u8; 32] = [9; 32];

/// A peer answering each request with the next response queued, through
/// the harness's script, and keeping each request's URL.
#[derive(Debug, Clone, Default)]
pub(crate) struct Peer(Script);

impl Peer {
    pub(crate) fn answering(answers: Vec<Response>) -> Self {
        Self(Script::answered(
            answers
                .into_iter()
                .map(|answer| (String::new(), answer))
                .collect(),
        ))
    }

    pub(crate) fn asked(&self) -> Vec<String> {
        self.0
            .seen()
            .iter()
            .map(|line| line.trim_start_matches("GET ").to_owned())
            .collect()
    }
}

pub(crate) fn config(peer: &str, source: &str) -> Config {
    routed(peer, source, "\"up\"")
}

/// [`config`], its route taking the sources `sources` lists.
pub(crate) fn routed(peer: &str, source: &str, sources: &str) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[federation]
domain = "herald.self"
keys = [{{ secret = "HERALD_KEY" }}]
[peers.upstream]
url = "{PEER}"
keys = ["{}"]
{peer}
[peers.origin]
url = "https://origin.example"
keys = ["{}"]
rotation = "signed"
[destinations.mirror]
kind = "herald"
[sources.up]
kind = "herald"
{source}
[[routes]]
id = "mirror"
sources = [{sources}]
destination = "mirror"
private = false
"#,
        ed25519_public(&OLD),
        ed25519_public(&ORIGIN)
    );
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

pub(crate) const FROM_PEER: &str = "peer = \"upstream\"\nfeed = \"releases\"\nverify = \"hop\"";

/// What one pull of `config`'s source did, its answers coming from `peer`.
pub(crate) fn pulled(config: &Config, store: &Failing, peer: &Peer) -> Sweep {
    let mut swept = swept(config, store, peer);
    assert_eq!(swept.len(), 1);
    swept.remove(0)
}

/// Everything one pull of `config`'s source did, its answers coming from
/// `peer`.
pub(crate) fn swept(config: &Config, store: &Failing, peer: &Peer) -> Vec<Sweep> {
    rounded(config, store, peer)
        .into_iter()
        .map(|swept| {
            assert_eq!(swept.source, SourceName::new("up"));
            swept.outcome
        })
        .collect()
}

/// Everything one round pulling `config`'s source did, its answers coming
/// from `peer`.
pub(crate) fn rounded(config: &Config, store: &Failing, peer: &Peer) -> Vec<Swept> {
    let secrets = FakeSecrets::default().with("HERALD_KEY", "federation-key-of-32-bytes!!!!!!");
    let prepared = pollster::block_on(prepare(config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(peer.0.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store,
        clock: &clock,
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    pollster::block_on(poll(config, &connected, &ports, &FakeVerifier, "r1"))
}

/// `body`, answering a GET of `url`, signed by `key`.
pub(crate) fn signed(url: &str, body: &str, key: &[u8; 32]) -> Response {
    answered(url, (200, body), key, None)
}

/// `body`, answering a GET of `url` with `status`, signed by `key`, over
/// the entity tag `etag` too where it is given.
pub(crate) fn answered(
    url: &str,
    (status, body): (u16, &str),
    key: &[u8; 32],
    etag: Option<&str>,
) -> Response {
    let body = body.as_bytes().to_vec();
    let mut headers = vec![
        (CONTENT_TYPE.to_owned(), "application/json".to_owned()),
        (CONTENT_DIGEST.to_owned(), content_digest(&body)),
    ];
    let mut components = demanded().to_vec();
    if let Some(etag) = etag {
        headers.push((ETAG.to_owned(), etag.to_owned()));
        components.push(Component::Field(ETAG.to_owned()));
    }
    let now = NOON.as_second();
    let covered = Covered {
        components,
        created: now,
        expires: now + 300,
        nonce: None,
        keyid: method(DOMAIN, key),
    };
    let message = Message {
        method: "GET",
        url,
        status: Some(status),
        fields: &headers,
    };
    let base = covered.base(&message).unwrap();
    headers.extend(covered.fields(&fake_ed25519(key, &base)));
    Response {
        status,
        headers,
        body,
    }
}

/// `envelope` proved by `key` of the instance on `domain`.
pub(crate) fn proved(envelope: &Value, domain: &str, key: &[u8; 32]) -> Value {
    proved_at(envelope, (domain, key), NOON)
}

/// `envelope` proved by `key` of the instance on `domain`, as made at
/// `created`.
pub(crate) fn proved_at(
    envelope: &Value,
    (domain, key): (&str, &[u8; 32]),
    created: jiff::Timestamp,
) -> Value {
    let unproved = proof::unproved(envelope, &method(domain, key), created).unwrap();
    let signature = fake_ed25519(key, unproved.message());
    unproved.proved(&signature)
}

/// The envelope of the release `identity`, made by the instance whose
/// discovery document is `path`'s first.
pub(crate) fn envelope(identity: &str, path: &[&str]) -> Value {
    json!({
        "identity": identity,
        "kind": "release",
        "project": "gate",
        "title": format!("gate {identity}"),
        "body": "the gate",
        "links": { "release": "https://forge.example/gate/releases/v1" },
        "attributes": { "version": "v1" },
        "occurred": "2026-11-17T10:00:00Z",
        "visibility": "public",
        "type": "herald:Event",
        "path": path,
    })
}

/// The feed's first page listing `items`, signed by `key`.
pub(crate) fn page(items: &[(u64, Value)], key: &[u8; 32]) -> Response {
    signed(FEED, &listing(items), key)
}

/// The body of the feed's first page listing `items`.
pub(crate) fn listing(items: &[(u64, Value)]) -> String {
    let items: Vec<Value> = items
        .iter()
        .map(|(sequence, envelope)| json!({ "sequence": sequence, "envelope": envelope }))
        .collect();
    json!({ "id": FEED, "feed": FEED, "items": items }).to_string()
}

/// The peer's own release `identity`, proved by `key`.
pub(crate) fn release(identity: &str, key: &[u8; 32]) -> Value {
    proved(&envelope(identity, &[DISCOVERY]), DOMAIN, key)
}

/// The peer's discovery document listing `set`, proved by `by` where it
/// gives one, its answer signed by `signer`.
pub(crate) fn discovered(set: &KeySet, by: Option<&[u8; 32]>, signer: &[u8; 32]) -> Response {
    let keys = by.map_or_else(
        || set.document(DOMAIN),
        |by| proved(&set.document(DOMAIN), DOMAIN, by),
    );
    let body =
        json!({ "version": 1, "keys": keys, "features": ["feeds"], "feeds": [] }).to_string();
    signed(DISCOVERY, &body, signer)
}

pub(crate) fn cursor(store: &Failing) -> Option<Cursor> {
    pollster::block_on(store.inner.cursor(&SourceName::new("up")))
        .unwrap()
        .map(|record| record.value)
}

pub(crate) fn announced(sweep: &Sweep) -> Vec<String> {
    let Sweep::Polled { announced, .. } = sweep else {
        panic!("{sweep:?}");
    };
    announced
        .iter()
        .map(|fresh| {
            assert!(
                fresh
                    .announced
                    .iter()
                    .all(|route| matches!(route.outcome, Outcome::Sent { .. })),
                "{fresh:?}"
            );
            fresh.event.identity().as_str().to_owned()
        })
        .collect()
}

#[test]
fn a_first_pull_is_quiet_and_a_later_one_announces_what_is_new() {
    let config = config("", FROM_PEER);
    let store = healthy();
    let first = Peer::answering(vec![page(
        &[(2, release("a/2", &OLD)), (1, release("a/1", &OLD))],
        &OLD,
    )]);
    assert_eq!(
        pulled(&config, &store, &first),
        Sweep::Polled {
            first: true,
            found: 2,
            announced: Vec::new(),
            cursor: Remembered::Advanced,
        }
    );
    assert_eq!(first.asked(), [FEED]);
    assert_eq!(cursor(&store).unwrap().sequence, Some(2));
    let later = Peer::answering(vec![page(
        &[(3, release("a/3", &OLD)), (2, release("a/2", &OLD))],
        &OLD,
    )]);
    assert_eq!(
        announced(&pulled(&config, &store, &later)),
        ["herald/upstream/a/3"]
    );
    assert_eq!(cursor(&store).unwrap().sequence, Some(3));
}

#[test]
fn an_envelope_another_peer_made_is_taken_by_hop_under_that_peer() {
    let config = config("", FROM_PEER);
    let store = healthy();
    let relayed = proved(
        &envelope(
            "o/1",
            &["https://origin.example/.well-known/herald", DISCOVERY],
        ),
        "origin.example",
        &ORIGIN,
    );
    pulled(
        &config,
        &store,
        &Peer::answering(vec![page(&[(1, release("a/1", &OLD))], &OLD)]),
    );
    let later = Peer::answering(vec![page(&[(2, relayed), (1, release("a/1", &OLD))], &OLD)]);
    assert_eq!(
        announced(&pulled(&config, &store, &later)),
        ["herald/origin/o/1"]
    );
}

#[test]
fn a_source_pinning_its_own_keys_pulls_within_the_bounds_herald_ships() {
    let source = format!("url = \"{FEED}\"\nkeys = [\"{}\"]", ed25519_public(&NEW));
    let config = config("", &source);
    let store = healthy();
    let first = Peer::answering(vec![page(&[(1, release("a/1", &NEW))], &NEW)]);
    assert!(matches!(
        pulled(&config, &store, &first),
        Sweep::Polled { found: 1, .. }
    ));
    let later = Peer::answering(vec![page(&[(2, release("a/2", &NEW))], &NEW)]);
    assert_eq!(
        announced(&pulled(&config, &store, &later)),
        ["herald/peer.example/a/2"]
    );
}

#[test]
fn a_pull_that_reads_nothing_stops_saying_why() {
    let config = config("", FROM_PEER);
    assert_eq!(
        pulled(&config, &healthy(), &Peer::default()),
        Sweep::Stopped(Stop::Unreachable(format!(
            "the host could not be reached: nothing scripted for GET {FEED}"
        )))
    );
    let stranger = Peer::answering(vec![page(&[(1, release("a/1", &NEW))], &NEW)]);
    assert_eq!(
        pulled(&config, &healthy(), &stranger),
        Sweep::Stopped(Stop::Source(format!(
            "the feed's answer is signed by {}, a key not trusted for the peer",
            method(DOMAIN, &NEW)
        )))
    );
    let mut unpeered = config.clone();
    unpeered.peers.remove("upstream");
    assert_eq!(
        pulled(&unpeered, &healthy(), &Peer::default()),
        Sweep::Stopped(Stop::Source(
            "there is no peer upstream in [peers] to pull from".to_owned()
        ))
    );
}

#[test]
fn a_release_entering_the_feed_again_changed_is_an_edit() {
    let config = config("", FROM_PEER);
    let store = healthy();
    let signalled = |body: &str, minutes: i64| {
        let mut released = envelope("a/1", &[DISCOVERY]);
        released["attributes"]["signal"] = json!("release");
        released["body"] = json!(body);
        proved_at(
            &released,
            (DOMAIN, &OLD),
            NOON + jiff::SignedDuration::from_mins(minutes),
        )
    };
    pulled(
        &config,
        &store,
        &Peer::answering(vec![page(&[(1, signalled("the gate", 0))], &OLD)]),
    );
    let edited = Peer::answering(vec![page(&[(2, signalled("the gate, fixed", 1))], &OLD)]);
    let Sweep::Polled { announced, .. } = pulled(&config, &store, &edited) else {
        panic!("a pull");
    };
    assert_eq!(announced.len(), 1);
    assert!(announced[0].event.edited());
    let again = Peer::answering(vec![page(&[(3, signalled("the gate, fixed", 2))], &OLD)]);
    let Sweep::Polled { announced, .. } = pulled(&config, &store, &again) else {
        panic!("a pull");
    };
    assert_eq!(announced, []);
}
