use herald_config::{Config, load};
use herald_core::{Chore, Identity, Key, Marker, RouteId, Schedule, SourceName, Step};
use herald_flows::{Chored, Outcome, Ports, Reached, announce, housekeep, prepare};
use herald_ports::{Markers as _, Store as _};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};

use crate::harness::*;

fn mastodon(store: &str, limit: usize) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "{store}"
[destinations.social]
kind = "mastodon"
server = "https://mastodon.example"
token = {{ secret = "HERALD_MASTODON_TOKEN" }}
[sources.gate]
kind = "github"
repository = "o/r"
[[routes]]
id = "tooted"
sources = ["gate"]
destination = "social"
template = "t"
visibility = "unlisted"
limit = {limit}
[templates.t]
title = "{{{{ event.title }}}}"
body = "{{{{ event.body }}}}"
"#
    );
    load("herald.toml", &text, &|_| Err(String::new())).unwrap()
}

fn token() -> FakeSecrets {
    FakeSecrets::default().with("HERALD_MASTODON_TOKEN", "t0ken")
}

const ACCOUNT: (&str, u16, &str) = ("/api/v1/accounts/verify_credentials", 200, r#"{"id":"42"}"#);
const NOTHING_POSTED: (&str, u16, &str) = (
    "/api/v1/accounts/42/statuses?limit=40&exclude_reblogs=true",
    200,
    "[]",
);

#[test]
fn a_mastodon_route_posts_its_status_and_replies_its_overflow_under_it() {
    let config = mastodon("markers", 130);
    let script = Script::answering(&[
        ACCOUNT,
        NOTHING_POSTED,
        (
            "/api/v1/statuses",
            200,
            r#"{"id":"1","url":"https://mastodon.example/@h/1"}"#,
        ),
        ("/api/v1/statuses", 200, r#"{"id":"2"}"#),
    ]);
    let prepared = pollster::block_on(prepare(&config, &token(), &FakeSigner)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let marked = connected.marked(FakeSigner);
    let ports = Ports {
        store: &marked,
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
            steps: vec![Step::Post, Step::Chunk(1)]
        }
    );
    assert_eq!(
        script.seen()[2],
        "POST https://mastodon.example/api/v1/statuses"
    );
}

#[test]
fn a_mastodon_route_keeps_no_thread_chores_and_its_account_lists_no_threads() {
    let config = mastodon("sqlite", 500);
    let store = healthy();
    let thread = Schedule {
        key: Key::derive(&RouteId::new("tooted"), &Identity::new("1")),
        route: RouteId::new("tooted"),
        thread: "1".to_owned(),
        chore: Chore::Archive,
        due: NOON,
    };
    pollster::block_on(store.put_schedules(&[thread], &[])).unwrap();
    let script = Script::default();
    let prepared = pollster::block_on(prepare(&config, &token(), &FakeSigner)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let kept = pollster::block_on(housekeep(&connected, &store, &FakeClock::at(NOON))).unwrap();
    assert_eq!(
        kept[0].outcome,
        Chored::Dropped("Mastodon cannot archive or lock a thread".to_owned())
    );
    assert_eq!(script.seen(), Vec::<String>::new());
    let reached = Reached::Mastodon(herald_dest_fediverse::Mastodon::new(
        "https://mastodon.example",
        herald_ports::Secret::new("HERALD_MASTODON_TOKEN", "t0ken"),
        herald_dest_fediverse::RouteOptions::default(),
    ));
    assert_eq!(reached.api(), "https://mastodon.example");
    assert_eq!(
        reached.profile(),
        herald_dest_fediverse::profile(&herald_dest_fediverse::RouteOptions::default())
    );
    let listed = pollster::block_on(reached.posts(&script).threads(&Marker::of(&[1]))).unwrap();
    assert_eq!(listed, []);
}
