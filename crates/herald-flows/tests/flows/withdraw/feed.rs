//! A release withdrawn from a feed of herald's own: its envelope replaced
//! by a tombstone, and a re-created release entering again (ADR-0048).

use herald_config::{Config, Uses, load};
use herald_core::{DestinationName, Event, SourceName, Step};
use herald_flows::{Outcome, Ports, announce, prepare};
use herald_peers::feed::withdrawn;
use herald_ports::Published;
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};

use crate::harness::{Failing, Fault, NOON, Script, event, healthy, outcome};

fn config() -> Config {
    let text = r#"version = 1
[store]
kind = "sqlite"
[federation]
domain = "herald.example"
keys = [{ secret = "HERALD_KEY" }]
[markers]
key = { secret = "HERALD_MARKER" }
[destinations.releases]
kind = "herald"
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
[[routes]]
id = "fed"
sources = ["gate"]
destination = "releases"
withdraw = "delete"
"#;
    load(
        "herald.toml",
        text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

fn announced(store: &Failing, event: &Event) -> Outcome {
    let config = config();
    let secrets = FakeSecrets::default()
        .with("HERALD_KEY", "new-federation-key-of-32-bytes!!")
        .with("HERALD_MARKER", "marker-key");
    let prepared =
        pollster::block_on(prepare(&config, &secrets, &FakeSigner, Uses::Announces)).unwrap();
    let connected = prepared
        .connect(|_| Ok::<_, ()>(Script::default()))
        .unwrap();
    let ports = Ports {
        store,
        clock: &FakeClock::at(NOON),
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    let announced = pollster::block_on(announce(
        &config,
        &connected,
        &ports,
        &SourceName::new("gate"),
        event,
        "r1",
    ));
    outcome(&announced).clone()
}

#[test]
fn a_withdrawn_release_s_envelope_is_replaced_by_a_tombstone_and_a_re_created_one_enters_again() {
    let store = healthy();
    let posted = Outcome::Sent {
        steps: vec![Step::Post],
    };
    assert_eq!(announced(&store, &event("v1")), posted);
    let gone = Outcome::Sent {
        steps: vec![Step::Delete(0)],
    };
    assert_eq!(announced(&store, &event("v1").marked_withdrawn()), gone);
    let feed = pollster::block_on(store.inner.feed(&DestinationName::new("releases")))
        .unwrap()
        .unwrap()
        .value;
    assert_eq!(feed.newest, 2);
    assert!(withdrawn(&feed.items[0].envelope));
    assert_eq!(feed.items[0].envelope["identity"], "github.com/1/v1");
    assert_eq!(
        announced(&store, &event("v1").marked_withdrawn()),
        Outcome::Skipped(super::UNANNOUNCED.to_owned())
    );
    assert_eq!(announced(&store, &event("v1")), posted);
    let again = pollster::block_on(store.inner.feed(&DestinationName::new("releases")))
        .unwrap()
        .unwrap()
        .value;
    assert_eq!(again.newest, 3);
    assert_eq!(again.items[0].envelope["identity"], "github.com/1/v1");
    assert!(!withdrawn(&again.items[0].envelope));
}

#[test]
fn a_tombstone_the_store_cannot_write_stops_the_withdrawal() {
    let mut store = healthy();
    announced(&store, &event("v1"));
    store.fault = Fault::WritesFeeds;
    let stopped = announced(&store, &event("v1").marked_withdrawn());
    assert!(matches!(stopped, Outcome::Stopped { .. }), "{stopped:?}");
    let mut stale = healthy();
    announced(&stale, &event("v1"));
    stale.fault = Fault::ClaimConflicts;
    *stale.writes.lock().unwrap() = 0;
    assert_eq!(
        announced(&stale, &event("v1").marked_withdrawn()),
        Outcome::AlreadyAnnounced
    );
}

#[test]
fn a_feed_announces_a_release_under_its_own_identity_reading_its_entry_once() {
    let mut store = healthy();
    store.fault = Fault::ReadsOnce;
    assert_eq!(
        announced(&store, &event("v1")),
        Outcome::Sent {
            steps: vec![Step::Post]
        }
    );
}
