use std::sync::Mutex;

use herald_config::{Config, Source};
use herald_core::{Cursor, Identity, SourceName};
use herald_flows::{Fresh, Outcome, Ports, PrepareError, Remembered, Stop, Sweep, poll, prepare};
use herald_ports::Store as _;
use herald_testing::{FakeClock, FakeSigner, FakeStore};

use crate::harness::*;

const RELEASES: &str = "/repos/nightworksio/php-mutation-gate/releases?per_page=30";
const REPOSITORY: (&str, u16, &str) = (
    "/repos/nightworksio/php-mutation-gate",
    200,
    r#"{"id":1,"name":"php-mutation-gate"}"#,
);
const ETAG: (&str, &str) = ("etag", "W/\"2\"");

fn releases(versions: &[&str]) -> String {
    let releases: Vec<serde_json::Value> = versions
        .iter()
        .map(|version| {
            serde_json::json!({
                "tag_name": version,
                "name": format!("php-mutation-gate {version}"),
                "body": "## Added",
                "draft": false,
                "prerelease": false,
                "published_at": "2026-10-31T12:00:00Z",
                "html_url": format!("https://github.com/nightworksio/php-mutation-gate/releases/tag/{version}"),
            })
        })
        .collect();
    serde_json::Value::Array(releases).to_string()
}

fn announced() -> [(&'static str, u16, &'static str); 6] {
    [POSTED, CROSSPOSTED, THREADED, PINNED, WHO, NO_PINS]
}

fn backfilled(mut config: Config, backfill: usize) -> Config {
    if let Some(Source::GitHub(options)) = config.sources.values_mut().next() {
        options.backfill = backfill;
    }
    config
}

fn swept(config: &Config, store: &Failing, script: &Script) -> Sweep {
    let prepared = pollster::block_on(prepare(config, &secrets(), &FakeSigner)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store,
        clock: &clock,
        signer: &FakeSigner,
    };
    let mut swept = pollster::block_on(poll(config, &connected, &ports, "r1"));
    assert_eq!(swept.len(), 1);
    let swept = swept.remove(0);
    assert_eq!(swept.source, SourceName::new("gate"));
    swept.outcome
}

fn cursor(store: &Failing) -> Option<Cursor> {
    pollster::block_on(store.inner.cursor(&SourceName::new("gate")))
        .unwrap()
        .map(|record| record.value)
}

fn seen(versions: &[&str]) -> Vec<Identity> {
    versions
        .iter()
        .map(|version| Identity::new(format!("github.com/1/{version}")))
        .collect()
}

#[test]
fn a_first_poll_remembers_what_it_finds_and_a_later_one_announces_only_what_is_new() {
    let store = healthy();
    let config = config("");
    let two = releases(&["v2", "v1"]);
    let first = Script::answering(&[(RELEASES, 200, &two), REPOSITORY]).headed(&[ETAG]);
    assert_eq!(
        swept(&config, &store, &first),
        Sweep::Polled {
            first: true,
            found: 2,
            announced: Vec::new(),
            cursor: Remembered::Advanced,
        }
    );
    assert_eq!(
        cursor(&store),
        Some(Cursor {
            seen: seen(&["v2", "v1"]),
            etag: Some("W/\"2\"".to_owned()),
            last_modified: None,
        })
    );
    let unchanged = Script::answering(&[(RELEASES, 304, "")]);
    assert_eq!(swept(&config, &store, &unchanged), Sweep::Unchanged);
    let three = releases(&["v3", "v2", "v1"]);
    let mut answers = vec![(RELEASES, 200, three.as_str()), REPOSITORY];
    answers.extend(announced());
    let Sweep::Polled {
        first,
        found,
        announced,
        cursor: remembered,
    } = swept(&config, &store, &Script::answering(&answers))
    else {
        panic!("the source was not polled");
    };
    assert_eq!((first, found, remembered), (false, 3, Remembered::Advanced));
    assert_eq!(announced.len(), 1);
    assert_eq!(announced[0].event.version(), Some("v3"));
    assert_eq!(
        announced[0].announced[0].outcome,
        Outcome::Sent { steps: all() }
    );
    assert_eq!(cursor(&store).unwrap().seen, seen(&["v3", "v2", "v1"]));
}

#[test]
fn backfill_announces_the_newest_on_the_first_poll_the_oldest_first() {
    let store = healthy();
    let body = releases(&["v3", "v2", "v1"]);
    let mut answers = vec![(RELEASES, 200, body.as_str()), REPOSITORY];
    answers.extend(announced());
    answers.extend(announced());
    let Sweep::Polled { announced, .. } = swept(
        &backfilled(config(""), 2),
        &store,
        &Script::answering(&answers),
    ) else {
        panic!("the source was not polled");
    };
    let versions: Vec<_> = announced
        .iter()
        .map(|fresh: &Fresh| fresh.event.version())
        .collect();
    assert_eq!(versions, [Some("v2"), Some("v3")]);
}

#[test]
fn a_route_that_stops_leaves_the_cursor_so_the_next_poll_offers_the_event_again() {
    let store = healthy();
    let known = Cursor {
        seen: seen(&["v1"]),
        ..Cursor::default()
    };
    pollster::block_on(
        store
            .inner
            .put_cursor(&SourceName::new("gate"), &known, None),
    )
    .unwrap();
    let body = releases(&["v2", "v1"]);
    let script = Script::answering(&[
        (RELEASES, 200, &body),
        REPOSITORY,
        (
            "/channels/100/messages",
            403,
            r#"{"code":50013,"message":"Missing Permissions"}"#,
        ),
    ]);
    let Sweep::Polled {
        announced,
        cursor: remembered,
        ..
    } = swept(&config(""), &store, &script)
    else {
        panic!("the source was not polled");
    };
    assert_eq!(remembered, Remembered::Kept);
    assert!(matches!(
        announced[0].announced[0].outcome,
        Outcome::Stopped { .. }
    ));
    assert_eq!(cursor(&store), Some(known));
}

fn faulty(fault: Fault) -> Failing {
    Failing {
        fault,
        inner: FakeStore::default(),
        writes: Mutex::new(0),
    }
}

#[test]
fn a_cursor_that_cannot_be_written_or_moved_first_by_another_run_says_so() {
    let body = releases(&["v1"]);
    let answers = [(RELEASES, 200, body.as_str()), REPOSITORY];
    let unwritten = swept(
        &config(""),
        &faulty(Fault::WritesCursors),
        &Script::answering(&answers),
    );
    assert!(
        matches!(
            &unwritten,
            Sweep::Polled { cursor: Remembered::Failed(Stop::Store(reason)), .. }
                if reason == "the store could not be reached: broken"
        ),
        "{unwritten:?}"
    );
    let raced = swept(
        &config(""),
        &faulty(Fault::CursorConflicts),
        &Script::answering(&answers),
    );
    assert!(
        matches!(
            &raced,
            Sweep::Polled { cursor: Remembered::Failed(Stop::Store(reason)), .. }
                if reason == "another run moved the source's cursor first"
        ),
        "{raced:?}"
    );
}

#[test]
fn a_source_that_cannot_be_read_or_reached_stops_its_poll() {
    let config = config("");
    let source = |reason: &str| Sweep::Stopped(Stop::Source(reason.to_owned()));
    assert_eq!(
        swept(&config, &faulty(Fault::Cursors), &Script::default()),
        Sweep::Stopped(Stop::Store(
            "the store could not be reached: broken".to_owned()
        ))
    );
    assert!(matches!(
        swept(&config, &healthy(), &Script::default()),
        Sweep::Stopped(Stop::Unreachable(_))
    ));
    let body = releases(&["v1"]);
    assert!(matches!(
        swept(
            &config,
            &healthy(),
            &Script::answering(&[(RELEASES, 200, &body)])
        ),
        Sweep::Stopped(Stop::Unreachable(_))
    ));
    assert_eq!(
        swept(
            &config,
            &healthy(),
            &Script::answering(&[(RELEASES, 200, &body), (REPOSITORY.0, 404, "{}")])
        ),
        source("GitHub's repository was answered with 404")
    );
    assert_eq!(
        swept(
            &config,
            &healthy(),
            &Script::answering(&[(RELEASES, 500, "{}"), REPOSITORY])
        ),
        source("GitHub's releases was answered with 500")
    );
}

#[test]
fn a_source_token_is_resolved_with_the_rest_of_the_config() {
    let mut config = config("");
    if let Some(Source::GitHub(options)) = config.sources.values_mut().next() {
        options.token = Some(herald_core::SecretRef {
            secret: "GITHUB_TOKEN".to_owned(),
        });
    }
    assert!(matches!(
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner)),
        Err(PrepareError::Secret(_))
    ));
    let tokened = secrets().with("GITHUB_TOKEN", "github-token");
    assert!(pollster::block_on(prepare(&config, &tokened, &FakeSigner)).is_ok());
}
