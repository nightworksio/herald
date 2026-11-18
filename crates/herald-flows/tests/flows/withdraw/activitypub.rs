//! A Note withdrawn: its `Delete` with a `Tombstone` delivered to each
//! follower the Note reached, and the Note no longer served (ADR-0048).

use herald_config::{Config, Uses, load};
use herald_core::{Event, Key, RouteId, SourceName, Step};
use herald_flows::{Outcome, Ports, announce, prepare};
use herald_ports::{Notes, Subscriptions};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};

use crate::activitypub::{follower, toml_text};
use crate::harness::{Failing, Fault, NOON, Script, event, healthy, outcome};

fn config() -> Config {
    let text = toml_text().replace(
        "actor = \"php-mutation-gate\"\n",
        "actor = \"php-mutation-gate\"\nwithdraw = \"delete\"\n",
    );
    load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

fn announced(store: &Failing, script: &Script, event: &Event) -> Outcome {
    let config = config();
    let secrets = FakeSecrets::default().with("HERALD_ACTIVITYPUB_KEY", "the-key");
    let prepared =
        pollster::block_on(prepare(&config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
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
fn a_withdrawn_note_is_deleted_at_each_inbox_it_reached_and_no_longer_served() {
    let store = healthy();
    let inbox = "https://social.example/inbox";
    let reached = follower(
        "php-mutation-gate",
        "https://social.example/users/ana",
        Some(inbox),
    );
    pollster::block_on(store.subscribe(&reached)).unwrap();
    let script = Script::answering(&[(inbox, 202, ""), (inbox, 202, "")]);
    assert_eq!(
        announced(&store, &script, &event("v1")),
        Outcome::Sent {
            steps: vec![Step::Post, Step::Deliver(inbox.to_owned())]
        }
    );
    let later = follower(
        "php-mutation-gate",
        "https://other.example/users/cy",
        Some("https://other.example/inbox"),
    );
    pollster::block_on(store.subscribe(&later)).unwrap();
    let key = Key::derive(&RouteId::new("followed"), event("v1").identity());
    assert!(
        pollster::block_on(store.inner.note(&key))
            .unwrap()
            .is_some()
    );
    assert_eq!(
        announced(&store, &script, &event("v1").marked_withdrawn()),
        Outcome::Sent {
            steps: vec![Step::Delete(0), Step::Deliver(inbox.to_owned())]
        }
    );
    let deleted: serde_json::Value = serde_json::from_slice(&script.bodies()[1]).unwrap();
    assert_eq!(deleted["type"], "Delete");
    assert_eq!(deleted["object"]["type"], "Tombstone");
    assert_eq!(pollster::block_on(store.inner.note(&key)).unwrap(), None);
    assert_eq!(script.seen().len(), 2);
}

#[test]
fn a_note_the_store_cannot_forget_stops_the_withdrawal() {
    let mut store = healthy();
    let script = Script::default();
    announced(&store, &script, &event("v1"));
    store.fault = Fault::ForgetsNotes;
    let stopped = announced(&store, &script, &event("v1").marked_withdrawn());
    assert!(
        matches!(stopped, Outcome::Stopped { ref landed, .. } if landed == &[Step::Delete(0)]),
        "{stopped:?}"
    );
}
