use herald_config::load;
use herald_core::{Chore, SourceName};
use herald_flows::{Outcome, Ports, announce, prepare};
use herald_ports::Store as _;
use herald_testing::{FakeClock, FakeSigner};
use jiff::SignedDuration;

use crate::harness::*;

const NO_MESSAGES: (&str, u16, &str) = ("/channels/100/messages?limit=100", 200, "[]");

fn posted_by_herald() -> String {
    serde_json::json!([{
        "id": "1",
        "type": 0,
        "author": { "id": "99" },
        "embeds": [{ "url": format!("https://example.org/r#herald-{}", marks()) }],
        "flags": 1,
        "pinned": true,
        "timestamp": "2026-10-31T12:00:01+00:00",
        "thread": {
            "id": "7",
            "thread_metadata": { "archived": false, "locked": false }
        }
    }])
    .to_string()
}

#[test]
fn an_announcement_kept_only_in_its_post_is_found_there_by_the_next_run() {
    let config = config("");
    let script = Script::default();
    let prepared = pollster::block_on(prepare(&config, &secrets(), &FakeSigner)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let store = connected.marked(FakeSigner);
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
    };
    let source = SourceName::new("gate");
    let posted = posted_by_herald();
    script.answers.lock().unwrap().extend(
        [
            WHO,
            NO_MESSAGES,
            POSTED,
            CROSSPOSTED,
            THREADED,
            PINNED,
            WHO,
            NO_PINS,
            WHO,
            ("/channels/100/messages?limit=100", 200, posted.as_str()),
            ("/channels/7/messages?limit=100", 200, "[]"),
            WHO,
            ("/channels/100/messages?limit=100", 200, posted.as_str()),
        ]
        .map(|(line, status, body)| (line.to_owned(), status, body.to_owned())),
    );
    let first = pollster::block_on(announce(
        &config,
        &connected,
        &ports,
        &source,
        &event("v1"),
        "r1",
    ));
    assert_eq!(first[0].outcome, Outcome::Sent { steps: all() });
    let again = pollster::block_on(announce(
        &config,
        &connected,
        &ports,
        &source,
        &event("v1"),
        "r2",
    ));
    assert_eq!(again[0].outcome, Outcome::AlreadyAnnounced);
    let schedules = pollster::block_on(store.schedules()).unwrap();
    assert_eq!(schedules.len(), 1);
    assert_eq!(
        (schedules[0].thread.as_str(), schedules[0].chore),
        ("7", Chore::Archive)
    );
    assert_eq!(
        schedules[0].due,
        "2026-10-31T12:00:01Z"
            .parse::<jiff::Timestamp>()
            .unwrap()
            .checked_add(SignedDuration::from_hours(24 * 14))
            .unwrap()
    );
}

#[test]
fn a_route_that_cannot_read_its_posts_back_is_left_out() {
    let text = r#"version = 1
[store]
kind = "markers"
[destinations.hook]
kind = "discord"
webhook = { secret = "HOOK" }
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
[[routes]]
id = "releases"
sources = ["gate"]
destination = "hook"
template = "release"
[templates.release]
title = "t"
body = "b"
"#;
    let config = load("herald.toml", text, &|_| Err(String::new())).unwrap();
    let secrets = secrets().with("HOOK", "https://discord.com/api/webhooks/1/x");
    let prepared = pollster::block_on(prepare(&config, &secrets, &FakeSigner)).unwrap();
    let script = Script::default();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let store = connected.marked(FakeSigner);
    let key = herald_core::Key::derive(
        &herald_core::RouteId::new("releases"),
        &herald_core::Identity::new("github.com/1/v1"),
    );
    assert_eq!(pollster::block_on(store.entry(&key)), Ok(None));
    assert_eq!(pollster::block_on(store.schedules()), Ok(Vec::new()));
    assert_eq!(script.seen(), Vec::<String>::new());
}
