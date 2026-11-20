//! An event published to a feed of herald's own: as an envelope proved by
//! the instance's newest key, once for each revision, only where the
//! routing saw it as public, and withdrawn by a tombstone once it is not.

use herald_config::{Config, Uses, load};
use herald_core::{
    DestinationName, Entry, Event, Feed, Identity, Key, Reach, RouteId, SourceName, Step,
    Visibility,
};
use herald_flows::{Connected, Outcome, Ports, Stop, Within, announce, announce_within, prepare};
use herald_peers::feed::withdrawn;
use herald_peers::keys::method;
use herald_ports::{Ledger, Published};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};

use crate::harness::{Failing, Fault, NOON, Script, event, healthy, outcome};

/// The instance's newest key, as the fake signer reads an Ed25519 key.
const KEY: &str = "new-federation-key-of-32-bytes!!";
/// Why an event that is not public enters no feed.
const PRIVATE: &str =
    "the event is not public, and a herald feed is served to anyone, so it enters none";

fn config(key: &str, extra: &str) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[federation]
domain = "herald.example"
keys = [{{ secret = "{key}" }}]
[markers]
key = {{ secret = "HERALD_MARKER" }}
[doctor]
private_to_public = "warn"
[destinations.releases]
kind = "herald"
{extra}
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
[[routes]]
id = "fed"
sources = ["gate"]
destination = "releases"
edits = "release"
"#
    );
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

fn secrets() -> FakeSecrets {
    FakeSecrets::default()
        .with("HERALD_KEY", KEY)
        .with("HERALD_MARKER", "marker-key")
        .with("HERALD_KEY_RSA", "not an Ed25519 key")
        .with("BAD", "unreadable")
}

struct World {
    config: Config,
    connected: Connected<Script>,
    store: Failing,
}

impl World {
    fn new(key: &str, extra: &str) -> Self {
        let config = config(key, extra);
        let prepared =
            pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Announces)).unwrap();
        let connected = prepared
            .connect(|_| Ok::<_, ()>(Script::default()))
            .unwrap();
        Self {
            config,
            connected,
            store: healthy(),
        }
    }

    fn within(&self, event: &Event, within: Within<'_>) -> Outcome {
        let ports = Ports {
            store: &self.store,
            clock: &FakeClock::at(NOON),
            signer: &FakeSigner,
            budget: herald_flows::Budget::UNBOUNDED,
        };
        let announced = pollster::block_on(announce_within(
            &self.config,
            &self.connected,
            &ports,
            &SourceName::new("gate"),
            event,
            within,
            "r1",
        ));
        outcome(&announced).clone()
    }

    fn announced(&self, event: &Event) -> Outcome {
        let ports = Ports {
            store: &self.store,
            clock: &FakeClock::at(NOON),
            signer: &FakeSigner,
            budget: herald_flows::Budget::UNBOUNDED,
        };
        let announced = pollster::block_on(announce(
            &self.config,
            &self.connected,
            &ports,
            &SourceName::new("gate"),
            event,
            "r1",
        ));
        outcome(&announced).clone()
    }

    fn feed(&self) -> Feed {
        pollster::block_on(self.store.inner.feed(&DestinationName::new("releases")))
            .unwrap()
            .map(|kept| kept.value)
            .unwrap_or_default()
    }

    fn entry(&self, version: &str) -> Option<Entry> {
        let key = Key::derive(
            &RouteId::new("fed"),
            &Identity::new(format!("github.com/1/{version}")),
        );
        pollster::block_on(self.store.inner.entry(&key))
            .unwrap()
            .map(|kept| kept.value)
    }
}

fn sent() -> Outcome {
    Outcome::Sent {
        steps: vec![Step::Post],
    }
}

fn skipped() -> Outcome {
    Outcome::Skipped(PRIVATE.to_owned())
}

fn stop(outcome: &Outcome) -> &Stop {
    match outcome {
        Outcome::Stopped { reason, .. } => reason,
        other => panic!("{other:?}"),
    }
}

fn changed(version: &str, change: impl FnOnce(&mut serde_json::Value)) -> Event {
    let mut written = serde_json::to_value(event(version)).unwrap();
    written["attributes"]["signal"] = "release".into();
    change(&mut written);
    serde_json::from_value(written).unwrap()
}

fn released(version: &str) -> Event {
    changed(version, |_| {})
}

fn edited(version: &str, body: &str) -> Event {
    changed(version, |written| written["body"] = body.into()).marked_edited()
}

#[test]
fn a_public_event_enters_the_feed_proved_by_the_instance_s_newest_key_once() {
    let world = World::new("HERALD_KEY", "");
    assert_eq!(world.announced(&event("v1")), sent());
    let feed = world.feed();
    assert_eq!(feed.newest, 1);
    let envelope = &feed.held("github.com/1/v1").unwrap().envelope;
    assert_eq!(envelope["type"], "herald:Event");
    assert_eq!(envelope["project"], "php-mutation-gate");
    assert_eq!(envelope["evidence"]["release"], "https://example.org/r");
    let key: [u8; 32] = KEY.as_bytes().try_into().unwrap();
    let proof = &envelope["proof"];
    assert_eq!(proof["verificationMethod"], method("herald.example", &key));
    assert_eq!(proof["created"], "2026-10-31T12:00:00Z");
    assert_eq!(proof["cryptosuite"], "eddsa-jcs-2022");
    assert!(
        proof["proofValue"]
            .as_str()
            .is_some_and(|value| value.starts_with('z'))
    );
    let entry = world.entry("v1").unwrap();
    assert_eq!(entry.message(&Step::Post), Some("1"));
    assert_eq!(entry.lease, None);
    assert_eq!(world.announced(&event("v1")), Outcome::AlreadyAnnounced);
    assert_eq!(world.feed(), feed);
}

#[test]
fn an_edited_event_enters_again_first_and_an_unknown_edit_enters_nothing() {
    let world = World::new("HERALD_KEY", "");
    assert_eq!(world.announced(&edited("v1", "fixed")), Outcome::Unedited);
    assert_eq!(world.announced(&released("v1")), sent());
    assert_eq!(world.announced(&released("v2")), sent());
    assert_eq!(world.announced(&edited("v1", "fixed")), sent());
    let feed = world.feed();
    assert_eq!(feed.newest, 3);
    assert_eq!(feed.items.len(), 2);
    assert_eq!(feed.items[0].envelope["body"], "fixed");
    assert_eq!(feed.items[0].sequence, 3);
    assert_eq!(world.entry("v1").unwrap().message(&Step::Post), Some("3"));
}

#[test]
fn an_event_the_feed_lists_already_is_landed_without_a_second_envelope() {
    let world = World::new("HERALD_KEY", "");
    assert_eq!(world.announced(&event("v1")), sent());
    let key = Key::derive(&RouteId::new("fed"), &Identity::new("github.com/1/v1"));
    let entry = pollster::block_on(world.store.inner.entry(&key))
        .unwrap()
        .unwrap();
    pollster::block_on(
        world
            .store
            .inner
            .put_entry(&Entry::new(key), Some(entry.version)),
    )
    .unwrap();
    assert_eq!(world.announced(&event("v1")), sent());
    assert_eq!(world.feed().newest, 1);
    assert_eq!(world.entry("v1").unwrap().message(&Step::Post), Some("1"));
}

#[test]
fn an_event_the_routing_saw_as_anything_but_public_never_enters() {
    let world = World::new("HERALD_KEY", "");
    for visibility in [Visibility::Private, Visibility::Internal] {
        let unseen = event("v1").seen_as(visibility);
        assert_eq!(world.announced(&unseen), skipped(), "{visibility:?}");
    }
    let member = Reach {
        routes: Vec::new(),
        only: false,
    };
    let within = Within {
        routes: None,
        reach: Some(&member),
        view: None,
        advised: &[],
    };
    assert_eq!(world.within(&event("v1"), within), skipped());
    assert_eq!(world.feed(), Feed::default());
    assert_eq!(world.entry("v1"), None);
    assert_eq!(
        world.announced(&event("v1").seen_as(Visibility::Public)),
        sent()
    );
}

#[test]
fn an_event_that_is_no_longer_public_is_withdrawn_by_a_tombstone_until_it_is_again() {
    let world = World::new("HERALD_KEY", "");
    assert_eq!(world.announced(&event("v1")), sent());
    let hidden = event("v1").seen_as(Visibility::Private);
    assert_eq!(world.announced(&hidden), skipped());
    let feed = world.feed();
    assert_eq!(feed.newest, 2);
    assert_eq!(feed.items.len(), 1);
    let tombstone = &feed.items[0].envelope;
    assert!(withdrawn(tombstone), "{tombstone}");
    assert_eq!(tombstone["identity"], "github.com/1/v1");
    assert_eq!(tombstone["deleted"], "2026-10-31T12:00:00Z");
    assert_eq!(tombstone["proof"]["created"], "2026-10-31T12:00:00Z");
    assert_eq!(world.entry("v1").unwrap().message(&Step::Post), None);
    assert_eq!(world.announced(&hidden), skipped());
    assert_eq!(world.feed().newest, 2);
    assert_eq!(world.announced(&event("v1")), sent());
    assert_eq!(world.feed().newest, 3);
    assert!(!withdrawn(&world.feed().items[0].envelope));
}

#[test]
fn a_feed_the_store_cannot_read_or_write_stops_the_route() {
    for fault in [
        Fault::ReadsFeeds,
        Fault::WritesFeeds,
        Fault::Reads,
        Fault::Claims,
    ] {
        let mut world = World::new("HERALD_KEY", "");
        world.store.fault = fault;
        let stopped = world.announced(&event("v1"));
        assert!(
            matches!(stop(&stopped), Stop::Store(_)),
            "{fault:?} {stopped:?}"
        );
    }
    let mut world = World::new("HERALD_KEY", "");
    world.store.fault = Fault::ConflictsFeeds;
    assert_eq!(
        stop(&world.announced(&event("v1"))),
        &Stop::Store(
            "the feed was written by another run each of 3 times herald wrote it".to_owned()
        )
    );
    let mut world = World::new("HERALD_KEY", "");
    world.store.fault = Fault::ClaimConflicts;
    assert_eq!(world.announced(&event("v1")), Outcome::AlreadyAnnounced);
    let mut world = World::new("HERALD_KEY", "");
    world.store.fault = Fault::KeepsNoPublished;
    assert!(matches!(
        stop(&world.announced(&event("v1"))),
        Stop::Store(_)
    ));
}

#[test]
fn a_tombstone_the_store_cannot_read_or_write_stops_the_route() {
    for fault in [Fault::ReadsFeeds, Fault::WritesFeeds, Fault::Releases] {
        let mut world = World::new("HERALD_KEY", "");
        assert_eq!(world.announced(&event("v1")), sent());
        world.store.fault = fault;
        let stopped = world.announced(&event("v1").seen_as(Visibility::Private));
        assert!(
            matches!(stop(&stopped), Stop::Store(_)),
            "{fault:?} {stopped:?}"
        );
    }
    let mut world = World::new("HERALD_KEY", "");
    world.store.fault = Fault::Reads;
    let stopped = world.announced(&event("v1").seen_as(Visibility::Private));
    assert!(matches!(stop(&stopped), Stop::Store(_)), "{stopped:?}");
}

#[test]
fn an_envelope_no_key_can_prove_or_too_long_to_fit_stops_the_route() {
    for (key, why) in [
        (
            "HERALD_KEY_RSA",
            "the federation key HERALD_KEY_RSA is not an Ed25519 key",
        ),
        (
            "BAD",
            "the secret BAD is not a key for this signature; give a private key as PEM, PKCS#8 or an RSA key's PKCS#1",
        ),
    ] {
        let world = World::new(key, "");
        assert_eq!(
            stop(&world.announced(&event("v1"))),
            &Stop::Proof(why.to_owned()),
            "{key}"
        );
        assert_eq!(world.feed(), Feed::default());
    }
    let world = World::new("HERALD_KEY", "envelope = \"1KiB\"");
    let long = changed("v1", |written| written["title"] = "t".repeat(2000).into());
    let stopped = world.announced(&long);
    assert!(
        matches!(stop(&stopped), Stop::Refused(why) if why.contains("with its body cut")),
        "{stopped:?}"
    );
}
