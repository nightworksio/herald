use herald_config::{Config, load};
use herald_core::{
    Chore, DestinationName, Identity, Key, Marker, RouteId, Schedule, SourceName, Step,
    Subscription,
};
use herald_flows::{Chored, Outcome, Ports, Reached, announce, housekeep, prepare};
use herald_ports::{Markers as _, MarkersError, Store as _};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};

use crate::harness::*;

fn activitypub() -> Config {
    let text = r#"version = 1
[store]
kind = "sqlite"
[destinations.fedi]
kind = "activitypub"
domain = "herald.example"
key = { secret = "HERALD_ACTIVITYPUB_KEY" }
[sources.gate]
kind = "github"
repository = "o/r"
[[routes]]
id = "followed"
sources = ["gate"]
destination = "fedi"
template = "t"
actor = "php-mutation-gate"
[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
"#;
    load("herald.toml", text, &|_| Err(String::new())).unwrap()
}

fn follower(actor: &str, follower: &str, inbox: Option<&str>) -> Subscription {
    Subscription {
        destination: DestinationName::new("fedi"),
        project: actor.to_owned(),
        subscriber: follower.to_owned(),
        inbox: inbox.map(ToOwned::to_owned),
    }
}

fn announced(store: &Failing, script: &Script) -> Outcome {
    let config = activitypub();
    let secrets = FakeSecrets::default().with("HERALD_ACTIVITYPUB_KEY", "the-key");
    let prepared = pollster::block_on(prepare(&config, &secrets, &FakeSigner)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let ports = Ports {
        store,
        clock: &FakeClock::at(NOON),
        signer: &FakeSigner,
    };
    let announced = pollster::block_on(announce(
        &config,
        &connected,
        &ports,
        &SourceName::new("gate"),
        &event("v1"),
        "r1",
    ));
    outcome(&announced).clone()
}

#[test]
fn an_announcement_is_delivered_once_to_each_inbox_its_actor_s_followers_share() {
    let store = healthy();
    for subscription in [
        follower(
            "php-mutation-gate",
            "https://social.example/users/ana",
            Some("https://social.example/inbox"),
        ),
        follower(
            "php-mutation-gate",
            "https://social.example/users/bo",
            Some("https://social.example/inbox"),
        ),
        follower(
            "php-mutation-gate",
            "https://other.example/users/cy",
            Some("https://other.example/users/cy/inbox"),
        ),
        follower("php-mutation-gate", "https://lost.example/users/di", None),
        follower(
            "another",
            "https://third.example/users/ed",
            Some("https://third.example/inbox"),
        ),
    ] {
        pollster::block_on(store.subscribe(&subscription)).unwrap();
    }
    let script = Script::answering(&[
        ("https://other.example/users/cy/inbox", 202, ""),
        ("https://social.example/inbox", 202, ""),
    ]);
    assert_eq!(
        announced(&store, &script),
        Outcome::Sent {
            steps: vec![
                Step::Post,
                Step::Deliver("https://other.example/users/cy/inbox".to_owned()),
                Step::Deliver("https://social.example/inbox".to_owned()),
            ]
        }
    );
    let created: serde_json::Value = serde_json::from_slice(&script.bodies()[0]).unwrap();
    assert_eq!(created["type"], "Create");
    assert_eq!(
        created["actor"],
        "https://herald.example/users/php-mutation-gate"
    );
}

#[test]
fn an_actor_no_one_follows_posts_its_note_and_delivers_nothing() {
    let store = healthy();
    let script = Script::default();
    assert_eq!(
        announced(&store, &script),
        Outcome::Sent {
            steps: vec![Step::Post]
        }
    );
    assert_eq!(script.seen(), Vec::<String>::new());
}

#[test]
fn an_actor_whose_followers_cannot_be_read_stops_before_anything_is_sent() {
    let mut store = healthy();
    store.fault = Fault::Subscribers;
    let script = Script::default();
    let Outcome::Stopped { reason, .. } = announced(&store, &script) else {
        panic!("the announcement stops");
    };
    assert_eq!(
        reason,
        herald_flows::Stop::Store("the store could not be reached: broken".to_owned())
    );
    assert_eq!(script.seen(), Vec::<String>::new());
}

#[test]
fn an_actor_reads_no_posts_back_and_delivers_to_its_own_followers() {
    let reached = Reached::ActivityPub(herald_dest_fediverse::ActivityPub::new(
        "herald.example",
        herald_ports::Secret::new("HERALD_ACTIVITYPUB_KEY", "the-key"),
        herald_dest_fediverse::ActorRouteOptions {
            actor: "php-mutation-gate".to_owned(),
            content_warning: None,
        },
    ));
    let script = Script::default();
    let posts = reached.posts(&script);
    let unread = MarkersError::Unreadable {
        reason: "herald's ActivityPub actor reads no posts back".to_owned(),
    };
    let key = Key::derive(&RouteId::new("followed"), &Identity::new("1"));
    assert_eq!(
        pollster::block_on(posts.entry(&key, &Marker::of(&[1]))),
        Err(unread.clone())
    );
    assert_eq!(
        pollster::block_on(posts.threads(&Marker::of(&[1]))),
        Err(unread)
    );
    assert_eq!(reached.follows(), Some("php-mutation-gate"));
    assert_eq!(reached.api(), herald_ports::PUBLIC);
    assert_eq!(reached.profile(), herald_dest_fediverse::actor_profile());
    assert_eq!(script.seen(), Vec::<String>::new());
}

#[test]
fn an_actor_keeps_no_thread_chores() {
    let config = activitypub();
    let store = healthy();
    let thread = Schedule {
        key: Key::derive(&RouteId::new("followed"), &Identity::new("1")),
        route: RouteId::new("followed"),
        thread: "1".to_owned(),
        chore: Chore::Archive,
        due: NOON,
    };
    pollster::block_on(store.put_schedules(&[thread], &[])).unwrap();
    let script = Script::default();
    let secrets = FakeSecrets::default().with("HERALD_ACTIVITYPUB_KEY", "the-key");
    let prepared = pollster::block_on(prepare(&config, &secrets, &FakeSigner)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let kept = pollster::block_on(housekeep(&connected, &store, &FakeClock::at(NOON))).unwrap();
    assert_eq!(
        kept[0].outcome,
        Chored::Dropped("herald's ActivityPub actor cannot archive or lock a thread".to_owned())
    );
    assert_eq!(script.seen(), Vec::<String>::new());
}
