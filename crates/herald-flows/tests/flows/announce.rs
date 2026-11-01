use std::sync::Mutex;

use herald_config::{Config, load};
use herald_core::{Chore, Entry, Identity, Key, RouteId, SourceName, Step};
use herald_flows::{Outcome, Ports, PrepareError, Stop, announce, prepare};
use herald_ports::Store as _;
use herald_testing::{FakeClock, FakeSecrets, FakeSigner, FakeStore};
use jiff::SignedDuration;

use crate::harness::*;

#[test]
fn an_event_is_announced_once_and_a_rerun_sends_nothing() {
    let store = healthy();
    let clock = FakeClock::at(NOON);
    let script = Script::answering(&[POSTED, CROSSPOSTED, THREADED, PINNED, WHO, NO_PINS]);
    let config = config("");
    let first = pollster::block_on(run(&config, &store, &clock, &script, "v1", "r1"));
    assert_eq!(first[0].route, RouteId::new("releases"));
    assert_eq!(outcome(&first), &Outcome::Sent { steps: all() });
    let key = Key::derive(&RouteId::new("releases"), &Identity::new("github.com/1/v1"));
    let entry = pollster::block_on(store.entry(&key))
        .unwrap()
        .unwrap()
        .value;
    assert_eq!(entry.lease, None);
    let schedules = pollster::block_on(store.schedules()).unwrap();
    assert_eq!(schedules.len(), 1);
    assert_eq!(schedules[0].chore, Chore::Archive);
    assert_eq!(schedules[0].thread, "7");
    let again = pollster::block_on(run(&config, &store, &clock, &script, "v1", "r2"));
    assert_eq!(outcome(&again), &Outcome::AlreadyAnnounced);
    assert_eq!(script.seen().len(), 6);
    assert!(script.seen()[0].starts_with("POST https://discord.com/api/v10/channels/100/messages"));
}

#[test]
fn an_event_no_route_takes_is_announced_nowhere() {
    let store = healthy();
    let config = config("");
    let prepared = pollster::block_on(prepare(&config, &secrets(), &FakeSigner)).unwrap();
    let connected = prepared
        .connect(|_| Ok::<_, ()>(Script::default()))
        .unwrap();
    let ports = Ports {
        store: &store,
        clock: &FakeClock::at(NOON),
        signer: &FakeSigner,
    };
    let announced = pollster::block_on(announce(
        &config,
        &connected,
        &ports,
        &SourceName::new("elsewhere"),
        &event("v1"),
        "r1",
    ));
    assert_eq!(announced, []);
}

#[test]
fn a_run_held_by_another_sends_nothing() {
    let store = healthy();
    let key = Key::derive(&RouteId::new("releases"), &Identity::new("github.com/1/v1"));
    let until = NOON.checked_add(SignedDuration::from_mins(5)).unwrap();
    pollster::block_on(store.put_entry(&Entry::new(key).claimed("other", until), None)).unwrap();
    let script = Script::default();
    let held = pollster::block_on(run(
        &config(""),
        &store,
        &FakeClock::at(NOON),
        &script,
        "v1",
        "r1",
    ));
    assert_eq!(
        outcome(&held),
        &Outcome::Held {
            by: "other".to_owned(),
            until
        }
    );
    assert_eq!(script.seen(), Vec::<String>::new());
}

#[test]
fn a_run_stopped_by_a_rate_limit_is_finished_by_the_next_after_it_asks_what_landed() {
    let store = healthy();
    let clock = FakeClock::at(NOON);
    let config = config("");
    let limited = Script::answering(&[
        POSTED,
        (
            "/channels/100/messages/1/crosspost",
            429,
            r#"{"retry_after":2.5}"#,
        ),
    ]);
    let first = pollster::block_on(run(&config, &store, &clock, &limited, "v1", "r1"));
    assert_eq!(
        outcome(&first),
        &Outcome::Stopped {
            landed: vec![Step::Post],
            remaining: all()[1..].to_vec(),
            reason: Stop::RetryAfter(SignedDuration::from_millis(2500)),
        }
    );
    let readback = r#"[{"id":"1","type":0,"author":{"id":"99"},"flags":1,"embeds":[{"url":"https://example.org/r#herald-ROUTE.MARK"}]}]"#;
    let marked = readback.replace("ROUTE.MARK", &marks());
    let finishing = Script::answering(&[
        WHO,
        ("/channels/100/messages?limit=100", 200, &marked),
        THREADED,
        PINNED,
        WHO,
        NO_PINS,
    ]);
    let second = pollster::block_on(run(&config, &store, &clock, &finishing, "v1", "r2"));
    assert_eq!(
        outcome(&second),
        &Outcome::Sent {
            steps: all()[1..].to_vec()
        }
    );
}

#[test]
fn a_refusal_or_an_unreachable_destination_stops_the_run() {
    let store = healthy();
    let clock = FakeClock::at(NOON);
    let config = config("");
    let refused = Script::answering(&[(
        "/channels/100/messages",
        403,
        r#"{"code":50013,"message":"Missing Permissions"}"#,
    )]);
    let first = pollster::block_on(run(&config, &store, &clock, &refused, "v1", "r1"));
    assert_eq!(
        outcome(&first),
        &Outcome::Stopped {
            landed: Vec::new(),
            remaining: all(),
            reason: Stop::Refused(
                "Discord refused it with 403: 50013 Missing Permissions".to_owned()
            ),
        }
    );
    let silent = Script::default();
    let second = pollster::block_on(run(&config, &store, &clock, &silent, "v2", "r1"));
    assert!(matches!(
        outcome(&second),
        Outcome::Stopped { reason: Stop::Unreachable(reason), .. } if reason.contains("nothing scripted")
    ));
}

#[test]
fn a_route_pinning_by_store_unpins_the_pin_the_store_remembers_and_remembers_its_own() {
    let store = healthy();
    pollster::block_on(store.put_pinned(&RouteId::new("releases"), "0")).unwrap();
    let config = pinned_by_store(config(""));
    let script = Script::answering(&[
        POSTED,
        CROSSPOSTED,
        THREADED,
        PINNED,
        ("/channels/100/messages/pins/0", 204, ""),
    ]);
    let announced = pollster::block_on(run(
        &config,
        &store,
        &FakeClock::at(NOON),
        &script,
        "v1",
        "r1",
    ));
    assert_eq!(outcome(&announced), &Outcome::Sent { steps: all() });
    assert_eq!(
        pollster::block_on(store.pinned(&RouteId::new("releases"))).unwrap(),
        Some("1".to_owned())
    );
}

fn pinned_by_store(mut config: Config) -> Config {
    config.routes[0].route.delivery.pin = Some(herald_core::Pinning::Store);
    config
}

fn token_named(name: &str) -> Config {
    let mut config = config("");
    if let Some(herald_config::Destination::Discord(options)) =
        config.destinations.values_mut().next()
    {
        options.token = Some(herald_core::SecretRef {
            secret: name.to_owned(),
        });
    }
    config
}

#[test]
fn an_event_a_template_cannot_render_or_a_marker_key_that_fails_stops_before_anything_is_sent() {
    let store = healthy();
    let clock = FakeClock::at(NOON);
    let script = Script::default();
    let rendered = pollster::block_on(run(&config(""), &store, &clock, &script, "boom", "r1"));
    assert!(matches!(
        outcome(&rendered),
        Outcome::Stopped {
            reason: Stop::Template(_),
            ..
        }
    ));
    let mut keyed = config("");
    keyed.marker_key = Some(herald_core::SecretRef {
        secret: "BAD".to_owned(),
    });
    let secrets = secrets().with("BAD", "x");
    let prepared = pollster::block_on(prepare(&keyed, &secrets, &FakeSigner)).unwrap();
    assert_eq!(prepared.derived(), Vec::<&RouteId>::new());
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
    };
    let marked = pollster::block_on(announce(
        &keyed,
        &connected,
        &ports,
        &SourceName::new("gate"),
        &event("v1"),
        "r1",
    ));
    assert!(matches!(
        outcome(&marked),
        Outcome::Stopped {
            reason: Stop::Marker(_),
            ..
        }
    ));
    assert_eq!(script.seen(), Vec::<String>::new());
}

#[test]
fn credentials_and_marker_keys_that_cannot_be_resolved_say_why() {
    let loaded = config("");
    let missing =
        pollster::block_on(prepare(&loaded, &FakeSecrets::default(), &FakeSigner)).unwrap_err();
    assert!(
        missing.to_string().contains("HERALD_DISCORD_TOKEN"),
        "{missing}"
    );
    let underived = pollster::block_on(prepare(
        &loaded,
        &FakeSecrets::default()
            .with("HERALD_DISCORD_TOKEN", "x")
            .with("BAD", "y"),
        &FakeSigner,
    ));
    assert!(underived.is_ok());
    let mut bad = token_named("BAD");
    let derived = pollster::block_on(prepare(
        &bad,
        &FakeSecrets::default().with("BAD", "y"),
        &FakeSigner,
    ))
    .unwrap_err();
    assert!(matches!(derived, PrepareError::Crypto(_)), "{derived}");
    assert!(derived.to_string().contains("BAD"), "{derived}");
    if let Some(herald_config::Destination::Discord(options)) = bad.destinations.values_mut().next()
    {
        options.token = None;
    }
    let none = pollster::block_on(prepare(&bad, &secrets(), &FakeSigner)).unwrap_err();
    assert_eq!(
        none.to_string(),
        "the destination \"discord\" names no credential"
    );
    let mut orphan = config("");
    orphan.destinations.clear();
    let lost = pollster::block_on(prepare(&orphan, &secrets(), &FakeSigner)).unwrap_err();
    assert_eq!(
        lost.to_string(),
        "the route \"releases\" names a destination the config does not have"
    );
    let prepared = pollster::block_on(prepare(&loaded, &secrets(), &FakeSigner)).unwrap();
    assert_eq!(prepared.derived(), [&RouteId::new("releases")]);
    assert_eq!(
        prepared
            .connect(|_| Err::<Script, _>("no client"))
            .unwrap_err(),
        "no client"
    );
}

#[test]
fn a_post_an_earlier_run_attempted_and_the_channel_does_not_show_is_sent_again() {
    let store = healthy();
    let clock = FakeClock::at(NOON);
    let config = config("");
    let unreachable = Script::default();
    let first = pollster::block_on(run(&config, &store, &clock, &unreachable, "v1", "r1"));
    assert!(matches!(
        outcome(&first),
        Outcome::Stopped {
            reason: Stop::Unreachable(_),
            ..
        }
    ));
    let resending = Script::answering(&[
        WHO,
        ("/channels/100/messages?limit=100", 200, "[]"),
        POSTED,
        CROSSPOSTED,
        THREADED,
        PINNED,
        WHO,
        NO_PINS,
    ]);
    let second = pollster::block_on(run(&config, &store, &clock, &resending, "v1", "r2"));
    assert_eq!(outcome(&second), &Outcome::Sent { steps: all() });
}

#[test]
fn a_webhook_destination_posts_and_opens_no_thread_to_keep() {
    let text = r#"version = 1
[store]
kind = "sqlite"
[destinations.hook]
kind = "discord"
webhook = { secret = "HOOK" }
[sources.gate]
kind = "github"
repository = "o/r"
[[routes]]
id = "hooked"
sources = ["gate"]
destination = "hook"
template = "t"
[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
"#;
    let config = load("herald.toml", text, &|_| Err(String::new())).unwrap();
    let store = healthy();
    let prepared = pollster::block_on(prepare(
        &config,
        &FakeSecrets::default().with("HOOK", "https://discord.com/api/webhooks/1/x"),
        &FakeSigner,
    ))
    .unwrap();
    let script = Script::answering(&[(
        "«secret:HOOK»?wait=true&with_components=true",
        200,
        r#"{"id":"5","channel_id":"6"}"#,
    )]);
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let ports = Ports {
        store: &store,
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
    assert_eq!(
        outcome(&announced),
        &Outcome::Sent {
            steps: vec![Step::Post]
        }
    );
    assert_eq!(pollster::block_on(store.schedules()).unwrap(), Vec::new());
}

#[test]
fn routes_and_sources_on_one_base_share_its_http() {
    let config = config(
        "[[routes]]\nid = \"second\"\nsources = [\"gate\"]\ndestination = \"discord\"\nchannel = \"200\"\ntemplate = \"release\"\n[sources.cli]\nkind = \"github\"\nrepository = \"cli/cli\"\n",
    );
    let prepared = pollster::block_on(prepare(&config, &secrets(), &FakeSigner)).unwrap();
    let mut made = Vec::new();
    let connected = prepared.connect(|base| {
        made.push(base.to_owned());
        Ok::<_, ()>(Script::default())
    });
    assert!(connected.is_ok());
    assert_eq!(
        made,
        ["https://discord.com/api/v10", "https://api.github.com"]
    );
    let refused = pollster::block_on(prepare(&config, &secrets(), &FakeSigner))
        .unwrap()
        .connect(|base| {
            if base.contains("github") {
                Err(base.to_owned())
            } else {
                Ok(Script::default())
            }
        });
    assert_eq!(refused.err().as_deref(), Some("https://api.github.com"));
}

fn failing(fault: Fault) -> Outcome {
    let store = Failing {
        fault,
        inner: FakeStore::default(),
        writes: Mutex::new(0),
    };
    let script = Script::answering(&[POSTED, CROSSPOSTED, THREADED, PINNED]);
    let config = pinned_by_store(config(""));
    let announced = pollster::block_on(run(
        &config,
        &store,
        &FakeClock::at(NOON),
        &script,
        "v1",
        "r1",
    ));
    announced[0].outcome.clone()
}

#[test]
fn a_store_that_fails_or_is_written_by_another_run_stops_the_run_and_says_where() {
    let reason = |outcome: Outcome| match outcome {
        Outcome::Stopped {
            reason: Stop::Store(reason),
            ..
        } => reason,
        other => panic!("{other:?}"),
    };
    for fault in [
        Fault::Reads,
        Fault::Claims,
        Fault::Schedules,
        Fault::Pins,
        Fault::RemembersPins,
        Fault::Releases,
    ] {
        assert_eq!(
            reason(failing(fault)),
            "the store could not be reached: broken",
            "{fault:?}"
        );
    }
    assert_eq!(
        reason(failing(Fault::ClaimConflicts)),
        "another run claimed the announcement first"
    );
    assert_eq!(
        reason(failing(Fault::Writes)),
        "another run wrote the announcement's ledger entry first"
    );
}
