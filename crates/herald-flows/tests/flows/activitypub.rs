use herald_config::{Config, Uses, load};
use herald_core::{
    Chore, DestinationName, Identity, Key, Marker, RouteId, Schedule, SourceName, Step,
    Subscription,
};
use herald_flows::{
    Chored, Findings, Outcome, Ports, PrepareError, Reached, announce, housekeep, keys, prepare,
};
use herald_ports::{Markers as _, MarkersError, Notes, Schedules, Subscriptions};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};

use crate::harness::*;

fn toml_text() -> &'static str {
    r#"version = 1
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
"#
}

pub(crate) fn activitypub() -> Config {
    load(
        "herald.toml",
        toml_text(),
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

/// The config of [`activitypub`], its actor's destination also set as
/// `policy` writes it.
pub(crate) fn governed(policy: &str) -> Config {
    let text = toml_text().replace(
        "key = { secret = \"HERALD_ACTIVITYPUB_KEY\" }\n",
        &format!("key = {{ secret = \"HERALD_ACTIVITYPUB_KEY\" }}\n{policy}\n"),
    );
    load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

pub(crate) fn follower(actor: &str, follower: &str, inbox: Option<&str>) -> Subscription {
    Subscription {
        destination: DestinationName::new("fedi"),
        project: actor.to_owned(),
        subscriber: follower.to_owned(),
        inbox: inbox.map(ToOwned::to_owned),
        pending: None,
    }
}

fn announced(store: &Failing, script: &Script) -> Outcome {
    announced_with(&activitypub(), store, script)
}

fn announced_with(config: &Config, store: &Failing, script: &Script) -> Outcome {
    let secrets = FakeSecrets::default().with("HERALD_ACTIVITYPUB_KEY", "the-key");
    let prepared = pollster::block_on(prepare(config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
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
fn an_announcement_is_delivered_to_no_follower_turned_away_or_waiting() {
    let store = healthy();
    for (who, inbox) in [
        (
            "https://social.example/users/ana",
            "https://social.example/inbox",
        ),
        (
            "https://eu.spam.example/users/x",
            "https://eu.spam.example/inbox",
        ),
    ] {
        pollster::block_on(store.subscribe(&follower("php-mutation-gate", who, Some(inbox))))
            .unwrap();
    }
    let script = Script::answering(&[("https://social.example/inbox", 202, "")]);
    assert_eq!(
        announced_with(&governed(r#"block = ["spam.example"]"#), &store, &script),
        Outcome::Sent {
            steps: vec![
                Step::Post,
                Step::Deliver("https://social.example/inbox".to_owned()),
            ]
        }
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
    let reached = Reached::new(herald_dest_fediverse::ActivityPub::new(
        "herald.example",
        "",
        herald_ports::Secret::new("HERALD_ACTIVITYPUB_KEY", "the-key"),
        herald_dest_fediverse::ActorRouteOptions {
            actor: "php-mutation-gate".to_owned(),
            content_warning: None,
            visibility: herald_dest_fediverse::Audience::Public,
            language: None,
        },
    ));
    let script = Script::default();
    let posts = reached.read(&script);
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
    let prepared =
        pollster::block_on(prepare(&config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let kept = pollster::block_on(housekeep(
        &connected,
        &store,
        &FakeClock::at(NOON),
        &herald_core::Retention::default(),
    ))
    .unwrap()
    .kept;
    assert_eq!(
        kept[0].outcome,
        Chored::Dropped("herald's ActivityPub actor cannot archive or lock a thread".to_owned())
    );
    assert_eq!(script.seen(), Vec::<String>::new());
}

fn keyed(secret: &str) -> Config {
    let text = toml_text().replace("HERALD_ACTIVITYPUB_KEY", secret);
    load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

#[test]
fn an_actor_whose_key_cannot_sign_stops_every_run_before_it_starts() {
    let secrets = FakeSecrets::default().with("BAD", "not a key");
    let Err(unkeyed) =
        pollster::block_on(prepare(&keyed("BAD"), &secrets, &FakeSigner, Uses::Polls))
    else {
        panic!("the run stops");
    };
    assert_eq!(
        unkeyed,
        PrepareError::Unkeyed {
            destination: "fedi".to_owned(),
            secret: "BAD".to_owned(),
        }
    );
    assert_eq!(
        unkeyed.to_string(),
        "the destination \"fedi\" signs with BAD, which is not an RSA private key of 2048 to 8192 bits in a PKCS#8 PEM; make one with `openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048`"
    );
}

#[test]
fn doctor_checks_each_actor_s_key_where_it_is_set() {
    let found = |config: &Config, secrets: &FakeSecrets| {
        pollster::block_on(keys(config, secrets, &FakeSigner))
    };
    let set = FakeSecrets::default().with("HERALD_ACTIVITYPUB_KEY", "the-key");
    assert_eq!(found(&activitypub(), &set), Findings::default());
    assert_eq!(
        found(&activitypub(), &FakeSecrets::default()),
        Findings::default()
    );
    let bad = found(&keyed("BAD"), &FakeSecrets::default().with("BAD", "x"));
    assert_eq!(bad.notes, Vec::<String>::new());
    assert_eq!(bad.mistakes.len(), 1);
    assert!(bad.mistakes[0].starts_with("the destination \"fedi\" signs with BAD,"));
    assert_eq!(
        found(&config(""), &FakeSecrets::default()),
        Findings::default()
    );
}

const CY: &str = "https://other.example/users/cy/inbox";
const SOCIAL: &str = "https://social.example/inbox";
const THIRD: &str = "https://third.example/inbox";

fn followed_at(store: &Failing, inboxes: &[&str]) {
    for (index, inbox) in inboxes.iter().enumerate() {
        let follower = format!("https://f{index}.example/users/f");
        pollster::block_on(store.subscribe(&self::follower(
            "php-mutation-gate",
            &follower,
            Some(inbox),
        )))
        .unwrap();
    }
}

fn deliver(inbox: &str) -> Step {
    Step::Deliver(inbox.to_owned())
}

#[test]
fn the_note_an_announcement_is_published_as_is_kept_to_be_served_by_its_id() {
    let store = healthy();
    let script = Script::default();
    assert_eq!(
        announced(&store, &script),
        Outcome::Sent {
            steps: vec![Step::Post]
        }
    );
    let key = Key::derive(&RouteId::new("followed"), event("v1").identity());
    let note: serde_json::Value =
        serde_json::from_str(&pollster::block_on(store.inner.note(&key)).unwrap().unwrap())
            .unwrap();
    assert_eq!(note["type"], "Note");
    assert_eq!(
        note["id"],
        format!("https://herald.example/users/php-mutation-gate/notes/{key}")
    );
}

#[test]
fn a_note_that_cannot_be_kept_stops_the_announcement_before_its_post_lands() {
    let mut store = healthy();
    store.fault = Fault::KeepsNotes;
    let Outcome::Stopped {
        landed,
        remaining,
        reason,
    } = announced(&store, &Script::default())
    else {
        panic!("the announcement stops");
    };
    assert_eq!(landed, Vec::<Step>::new());
    assert_eq!(remaining, vec![Step::Post, Step::Deliveries]);
    assert_eq!(
        reason,
        herald_flows::Stop::Store("the store could not be reached: broken".to_owned())
    );
}

#[test]
fn every_inbox_is_delivered_to_at_once_and_only_those_that_failed_remain() {
    let store = healthy();
    followed_at(&store, &[CY, SOCIAL, THIRD]);
    let script = Script::answering(&[(CY, 500, ""), (SOCIAL, 202, ""), (THIRD, 403, "")]);
    assert_eq!(
        announced(&store, &script),
        Outcome::Stopped {
            landed: vec![Step::Post, deliver(SOCIAL)],
            remaining: vec![deliver(CY), deliver(THIRD)],
            reason: herald_flows::Stop::RetryAfter(jiff::SignedDuration::from_secs(5)),
        }
    );
    assert_eq!(script.seen().len(), 3);
    let again = Script::answering(&[(CY, 202, ""), (THIRD, 202, "")]);
    assert_eq!(
        announced(&store, &again),
        Outcome::Sent {
            steps: vec![deliver(CY), deliver(THIRD)]
        }
    );
}

#[test]
fn deliveries_go_out_as_many_at_once_as_the_destination_allows() {
    let config = load(
        "herald.toml",
        &toml_text().replace(
            "key = { secret = \"HERALD_ACTIVITYPUB_KEY\" }",
            "key = { secret = \"HERALD_ACTIVITYPUB_KEY\" }\ndeliveries = 1",
        ),
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let store = healthy();
    followed_at(&store, &[CY, SOCIAL]);
    let script = Script::answering(&[(CY, 403, "")]);
    let Outcome::Stopped {
        landed, remaining, ..
    } = announced_with(&config, &store, &script)
    else {
        panic!("the announcement stops");
    };
    assert_eq!(landed, vec![Step::Post]);
    assert_eq!(remaining, vec![deliver(CY), deliver(SOCIAL)]);
    assert_eq!(script.seen().len(), 1);
}

#[test]
fn a_delivery_that_reaches_no_inbox_stops_as_unreachable() {
    let store = healthy();
    followed_at(&store, &[CY]);
    let Outcome::Stopped { reason, .. } = announced(&store, &Script::default()) else {
        panic!("the announcement stops");
    };
    assert!(matches!(reason, herald_flows::Stop::Unreachable(_)));
}

#[test]
fn deliveries_the_ledger_cannot_record_stop_with_every_one_of_them_remaining() {
    for fault in [Fault::AttemptsDeliveries, Fault::LandsDeliveries] {
        let mut store = healthy();
        store.fault = fault;
        followed_at(&store, &[CY, SOCIAL]);
        let script = Script::answering(&[(CY, 202, ""), (SOCIAL, 202, "")]);
        let Outcome::Stopped {
            landed,
            remaining,
            reason,
        } = announced(&store, &script)
        else {
            panic!("the announcement stops");
        };
        assert_eq!(landed, vec![Step::Post], "{fault:?}");
        assert_eq!(remaining, vec![deliver(CY), deliver(SOCIAL)], "{fault:?}");
        assert!(matches!(reason, herald_flows::Stop::Store(_)), "{fault:?}");
    }
}

#[test]
fn deliveries_whose_records_cannot_be_read_stop_before_any_is_sent() {
    let mut store = healthy();
    store.fault = Fault::ReadsDeliveries;
    followed_at(&store, &[CY, SOCIAL]);
    let script = Script::answering(&[(CY, 202, ""), (SOCIAL, 202, "")]);
    let Outcome::Stopped {
        landed,
        remaining,
        reason,
    } = announced(&store, &script)
    else {
        panic!("the announcement stops");
    };
    assert_eq!(landed, vec![Step::Post]);
    assert_eq!(remaining, vec![Step::Deliveries]);
    assert!(matches!(reason, herald_flows::Stop::Store(_)));
    assert_eq!(script.seen(), Vec::<String>::new());
}

#[test]
fn a_run_resumed_delivers_only_to_the_inboxes_it_kept_no_landing_for() {
    let mut store = healthy();
    store.fault = Fault::LandsOneDelivery;
    followed_at(&store, &[CY, SOCIAL]);
    let script = Script::answering(&[(CY, 202, ""), (SOCIAL, 202, "")]);
    let Outcome::Stopped {
        landed, remaining, ..
    } = announced(&store, &script)
    else {
        panic!("the announcement stops");
    };
    assert_eq!(landed, vec![Step::Post]);
    assert_eq!(remaining, vec![deliver(CY), deliver(SOCIAL)]);
    store.fault = Fault::Nothing;
    let again = Script::answering(&[(SOCIAL, 202, "")]);
    assert_eq!(
        announced(&store, &again),
        Outcome::Sent {
            steps: vec![deliver(SOCIAL)]
        }
    );
    assert_eq!(again.seen().len(), 1);
}
