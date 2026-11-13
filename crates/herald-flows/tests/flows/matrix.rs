use herald_config::{Config, Uses, load};
use herald_core::{
    Chore, DestinationName, Identity, Key, Marker, RouteId, Schedule, SourceName, Step,
    Subscription,
};
use herald_flows::{Chored, Outcome, Ports, Reached, announce, prepare};
use herald_ports::{Markers as _, Schedules, Subscriptions};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};

use crate::harness::*;

fn matrix(mentions: bool) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.room]
kind = "matrix"
homeserver = "https://matrix.example"
token = {{ secret = "HERALD_MATRIX_TOKEN" }}
[sources.gate]
kind = "github"
repository = "o/r"
[[routes]]
id = "roomed"
sources = ["gate"]
destination = "room"
template = "t"
room = "!releases:matrix.example"
mentions = {{ subscribers = {mentions} }}
[templates.t]
title = "{{{{ event.title }}}}"
body = "{{{{ event.body }}}}"
"#
    );
    load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

fn token() -> FakeSecrets {
    FakeSecrets::default().with("HERALD_MATRIX_TOKEN", "t0ken")
}

fn sent() -> String {
    let key = Key::derive(&RouteId::new("roomed"), &Identity::new("github.com/1/v1"));
    format!("/rooms/%21releases%3Amatrix.example/send/m.room.message/{key}")
}

fn subscribed(store: &Failing) {
    let subscription = Subscription {
        destination: DestinationName::new("room"),
        project: "php-mutation-gate".to_owned(),
        subscriber: "@ana:matrix.example".to_owned(),
        inbox: None,
        pending: None,
    };
    pollster::block_on(store.subscribe(&subscription)).unwrap();
}

fn announced(config: &Config, store: &Failing, script: &Script) -> Outcome {
    let prepared = pollster::block_on(prepare(config, &token(), &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let ports = Ports {
        store,
        clock: &FakeClock::at(NOON),
        signer: &FakeSigner,
    };
    let announced = pollster::block_on(announce(
        config,
        &connected,
        &ports,
        &SourceName::new("gate"),
        &event("v1"),
        "r1",
    ));
    outcome(&announced).clone()
}

fn mentioned(body: &[u8]) -> serde_json::Value {
    let content: serde_json::Value = serde_json::from_slice(body).unwrap();
    content["m.mentions"]["user_ids"].clone()
}

#[test]
fn a_matrix_route_mentions_the_subscribers_herald_keeps() {
    let store = healthy();
    subscribed(&store);
    let posted = sent();
    let script = Script::answering(&[(posted.as_str(), 200, r#"{"event_id":"$e1"}"#)]);
    assert_eq!(
        announced(&matrix(true), &store, &script),
        Outcome::Sent {
            steps: vec![Step::Post]
        }
    );
    assert_eq!(
        script.seen(),
        [format!(
            "PUT https://matrix.example/_matrix/client/v3{posted}"
        )]
    );
    assert_eq!(
        mentioned(&script.bodies()[0]),
        serde_json::json!(["@ana:matrix.example"])
    );
}

#[test]
fn subscribers_past_the_announcement_s_room_are_mentioned_in_a_message_after_it() {
    let store = healthy();
    for index in 0..300 {
        let subscription = Subscription {
            destination: DestinationName::new("room"),
            project: "php-mutation-gate".to_owned(),
            subscriber: format!("@user{index:03}:matrix.example"),
            inbox: None,
            pending: None,
        };
        pollster::block_on(store.subscribe(&subscription)).unwrap();
    }
    let posted = sent();
    let after = format!("{posted}.1");
    let script = Script::answering(&[
        (posted.as_str(), 200, r#"{"event_id":"$e1"}"#),
        (after.as_str(), 200, r#"{"event_id":"$e2"}"#),
    ]);
    assert_eq!(
        announced(&matrix(true), &store, &script),
        Outcome::Sent {
            steps: vec![Step::Post, Step::Chunk(1)]
        }
    );
    let bodies = script.bodies();
    let first = mentioned(&bodies[0]).as_array().unwrap().len();
    let rest = mentioned(&bodies[1]).as_array().unwrap().len();
    assert_eq!((first + rest, first > 100, rest > 100), (300, true, true));
    assert!(bodies.iter().all(|body| body.len() < 65_536));
}

#[test]
fn a_matrix_route_that_asks_for_no_subscribers_mentions_none() {
    let store = healthy();
    subscribed(&store);
    let posted = sent();
    let script = Script::answering(&[(posted.as_str(), 200, r#"{"event_id":"$e1"}"#)]);
    announced(&matrix(false), &store, &script);
    assert_eq!(mentioned(&script.bodies()[0]), serde_json::json!([]));
}

#[test]
fn a_matrix_route_whose_subscribers_cannot_be_read_stops_before_posting() {
    let mut store = healthy();
    store.fault = Fault::Subscribers;
    let script = Script::default();
    let Outcome::Stopped { reason, .. } = announced(&matrix(true), &store, &script) else {
        panic!("the announcement stops");
    };
    assert_eq!(
        reason,
        herald_flows::Stop::Store("the store could not be reached: broken".to_owned())
    );
    assert_eq!(script.seen(), Vec::<String>::new());
}

#[test]
fn a_matrix_route_keeps_no_thread_chores_and_its_room_lists_no_threads() {
    let config = matrix(false);
    let store = healthy();
    let thread = Schedule {
        key: Key::derive(&RouteId::new("roomed"), &Identity::new("1")),
        route: RouteId::new("roomed"),
        thread: "1".to_owned(),
        chore: Chore::Archive,
        due: NOON,
    };
    pollster::block_on(store.put_schedules(&[thread], &[])).unwrap();
    let script = Script::default();
    let prepared =
        pollster::block_on(prepare(&config, &token(), &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let kept = housekept(&connected, &store, &herald_core::Retention::default())
        .unwrap()
        .kept;
    assert_eq!(
        kept[0].outcome,
        Chored::Dropped("Matrix cannot archive or lock a thread".to_owned())
    );
    assert_eq!(script.seen(), Vec::<String>::new());
    let reached = Reached::new(herald_dest_matrix::Matrix::new(
        "https://matrix.example",
        herald_ports::Secret::new("HERALD_MATRIX_TOKEN", "t0ken"),
        herald_dest_matrix::RouteOptions {
            room: "!releases:matrix.example".to_owned(),
        },
    ));
    assert_eq!(reached.api(), "https://matrix.example");
    assert_eq!(reached.profile(), herald_dest_matrix::profile());
    let listed = pollster::block_on(reached.read(&script).threads(&Marker::of(&[1]))).unwrap();
    assert_eq!(listed, []);
    let key = Key::derive(&RouteId::new("roomed"), &Identity::new("1"));
    let read = pollster::block_on(reached.read(&script).entry(&key, &Marker::of(&[1])));
    let Err(herald_ports::MarkersError::Unreadable { reason }) = read else {
        panic!("{read:?}");
    };
    assert!(reason.contains("nothing scripted"), "{reason}");
    let marker = Marker::of(&[1]);
    let page = serde_json::json!({ "chunk": [{
        "event_id": "$post",
        "sender": "@herald:matrix.example",
        "type": "m.room.message",
        "content": { "io.nightworks.herald.marker": marker.as_str() },
    }] })
    .to_string();
    let cut_short = Script::answering(&[
        (
            "/account/whoami",
            200,
            r#"{"user_id":"@herald:matrix.example"}"#,
        ),
        ("/messages?dir=b&limit=100", 200, page.as_str()),
    ]);
    let read = pollster::block_on(reached.read(&cut_short).entry(&key, &marker));
    let Err(herald_ports::MarkersError::Unreadable { reason }) = read else {
        panic!("{read:?}");
    };
    assert!(reason.contains("nothing scripted"), "{reason}");
    assert_eq!(cut_short.seen().len(), 3);
}
