//! A living board keeps one post, edited each window where its destination
//! edits and posted anew where it cannot; a rule nudges each pull request
//! that waited past a stage once a stage and spell, reading each view once
//! a round with the reports (ADR-0056).

use herald_config::{Config, Uses, load};
use herald_core::SourceName;
use herald_flows::{Connected, Outcome, Ports, Remembered, Stop, Sweep, Swept, prepare, report};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;

use crate::harness::{Failing, NO_PINS, NOON, PINNED, POSTED, Script, WHO, healthy};

const GRAPHQL: &str = "https://api.github.com/graphql";
const EDITED: (&str, u16, &str) = (
    "/channels/1566200000000000100/messages/1",
    200,
    r#"{"id":"1"}"#,
);
const HOOK: &str = "https://hooks.slack.com/services/T0/B0/x";

fn config(extra: &str) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[destinations.slack]
kind = "slack"
webhook = {{ secret = "HERALD_SLACK_WEBHOOK" }}
[sources.open]
kind = "github"
repository = "o/open"
token = {{ secret = "HERALD_GITHUB_TOKEN" }}
[views.open]
sources = ["open"]
[run]
routes = 1
[templates.t]
title = "{{{{ view.name }}}}"
body = "rows:{{% for group in view.groups %}}{{% for row in group.rows %}} {{{{ row.title }}}}{{% endfor %}}{{% endfor %}}"
{extra}
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

/// A GraphQL page of `o/open`, seen as `visibility`, with one open pull
/// request titled `title`, last acted on at `updated`.
fn page(title: &str, visibility: &str, updated: &str) -> String {
    json!({ "data": { "repository": { "visibility": visibility, "pullRequests": { "nodes": [ {
        "number": 1, "title": title, "url": "https://github.com/o/open/pull/1",
        "isDraft": false, "createdAt": "2026-10-20T10:00:00Z", "updatedAt": updated,
        "author": { "__typename": "User", "login": "ana" },
        "reviewDecision": "REVIEW_REQUIRED",
        "reviewRequests": { "totalCount": 1, "nodes": [ { "requestedReviewer": { "login": "bo" } } ] }
    } ] } } } })
    .to_string()
}

fn round(config: &Config, store: &Failing, script: &Script, at: Timestamp) -> Vec<Swept> {
    let secrets = FakeSecrets::default()
        .with("HERALD_DISCORD_TOKEN", "hunter2")
        .with("HERALD_SLACK_WEBHOOK", HOOK)
        .with("HERALD_GITHUB_TOKEN", "ghp_x");
    let prepared = pollster::block_on(prepare(config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(at);
    let ports = Ports {
        store,
        clock: &clock,
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    pollster::block_on(report(config, &connected, &ports, "r1"))
}

fn hours(count: i64) -> Timestamp {
    NOON.checked_add(SignedDuration::from_hours(count)).unwrap()
}

/// Each announcement's identity, and what every route did with it.
fn announced(swept: &Swept) -> Vec<(String, Vec<(String, bool)>)> {
    let Sweep::Reported { announced, .. } = &swept.outcome else {
        panic!("{swept:?}");
    };
    announced
        .iter()
        .map(|fresh| {
            (
                fresh.event.identity().as_str().to_owned(),
                fresh
                    .announced
                    .iter()
                    .map(|route| {
                        (
                            route.route.as_str().to_owned(),
                            matches!(route.outcome, Outcome::Sent { .. }),
                        )
                    })
                    .collect(),
            )
        })
        .collect()
}

const BOARD: &str = r#"[boards.live]
view = "open"
every = "1h"
at = "00:00"
destination = "discord"
channel = "1566200000000000100"
template = "t"
"#;

#[test]
fn a_board_posts_and_pins_once_then_each_window_edits_its_post_where_it_changed() {
    let config = config(BOARD);
    let store = healthy();
    let first = Script::answering(&[
        (
            GRAPHQL,
            200,
            &page("Change", "PUBLIC", "2026-10-30T11:00:00Z"),
        ),
        POSTED,
        PINNED,
        WHO,
        NO_PINS,
    ]);
    let swept = round(&config, &store, &first, NOON);
    assert_eq!(
        announced(&swept[0]),
        [("report/live".to_owned(), vec![("live".to_owned(), true)])]
    );
    let unchanged = Script::answering(&[(
        GRAPHQL,
        200,
        &page("Change", "PUBLIC", "2026-10-30T11:00:00Z"),
    )]);
    let swept = round(&config, &store, &unchanged, hours(1));
    assert_eq!(
        announced(&swept[0]),
        [("report/live".to_owned(), vec![("live".to_owned(), false)])]
    );
    assert_eq!(unchanged.seen().len(), 1);
    let changed = Script::answering(&[
        (
            GRAPHQL,
            200,
            &page("Renamed", "PUBLIC", "2026-10-30T11:00:00Z"),
        ),
        EDITED,
    ]);
    let swept = round(&config, &store, &changed, hours(2));
    assert_eq!(
        announced(&swept[0]),
        [("report/live".to_owned(), vec![("live".to_owned(), true)])]
    );
    let edit = String::from_utf8(changed.bodies()[1].clone()).unwrap();
    assert!(edit.contains("Renamed"), "{edit}");
    assert!(
        changed.seen()[1].starts_with("PATCH "),
        "{:?}",
        changed.seen()
    );
}

#[test]
fn a_living_route_that_cannot_edit_posts_each_window_anew_beside_one_that_edits() {
    let config = config(&format!(
        "{BOARD}\n[boards.hooked]\nview = \"open\"\nevery = \"1h\"\nat = \"00:00\"\ndestination = \"slack\"\ntemplate = \"t\"\n"
    ));
    let store = healthy();
    let script = Script::answering(&[
        (
            GRAPHQL,
            200,
            &page("Change", "PUBLIC", "2026-10-30T11:00:00Z"),
        ),
        ("«secret:HERALD_SLACK_WEBHOOK»", 200, "ok"),
        POSTED,
        PINNED,
        WHO,
        NO_PINS,
    ]);
    let swept = round(&config, &store, &script, NOON);
    assert_eq!(swept[0].source, SourceName::new("hooked"));
    assert_eq!(
        announced(&swept[0]),
        [(
            "report/hooked/2026-10-31T12:00:00Z".to_owned(),
            vec![("hooked".to_owned(), true)]
        )]
    );
    assert_eq!(
        announced(&swept[1]),
        [("report/live".to_owned(), vec![("live".to_owned(), true)])]
    );
    assert_eq!(script.seen().len(), 6, "one read for both boards");
}

const RULE: &str = r#"[rules.waiting]
view = "open"
stages = ["1d", "3d"]
every = "1h"
at = "00:00"
[[routes]]
id = "nudges"
sources = ["waiting"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:nudge"
private = true
"#;

#[test]
fn a_rule_nudges_once_a_stage_and_spell_and_again_at_its_next_stage_or_spell() {
    let config = config(RULE);
    let store = healthy();
    let read = |updated: &str| (GRAPHQL, 200, page("Change", "PUBLIC", updated));
    let (graphql, status, body) = read("2026-10-30T11:00:00Z");
    let first = Script::answering(&[(graphql, status, &body), POSTED]);
    let swept = round(&config, &store, &first, NOON);
    assert_eq!(
        announced(&swept[0]),
        [(
            "rule/waiting/github.com/o/open/pull/1/2026-10-30T11:00:00Z/1d".to_owned(),
            vec![("nudges".to_owned(), true)]
        )]
    );
    let nudge = String::from_utf8(first.bodies()[1].clone()).unwrap();
    assert!(
        nudge.contains("o/open#1 has waited 1d") && nudge.contains("waiting on bo"),
        "{nudge}"
    );
    let again = Script::answering(&[(graphql, status, &body)]);
    let swept = round(&config, &store, &again, hours(1));
    assert_eq!(
        announced(&swept[0]),
        [(
            "rule/waiting/github.com/o/open/pull/1/2026-10-30T11:00:00Z/1d".to_owned(),
            vec![("nudges".to_owned(), false)]
        )]
    );
    assert_eq!(round(&config, &store, &Script::default(), hours(1)), []);
    let later = Script::answering(&[(graphql, status, &body), POSTED]);
    let swept = round(&config, &store, &later, hours(48));
    assert_eq!(
        announced(&swept[0])[0].0,
        "rule/waiting/github.com/o/open/pull/1/2026-10-30T11:00:00Z/3d"
    );
    let (graphql, status, body) = read("2026-11-02T11:00:00Z");
    let spell = Script::answering(&[(graphql, status, &body)]);
    let swept = round(&config, &store, &spell, hours(49));
    assert_eq!(announced(&swept[0]), []);
}

#[test]
fn a_private_pull_request_is_nudged_only_to_private_routes() {
    let config = config(&format!(
        "{RULE}[[routes]]\nid = \"public\"\nsources = [\"waiting\"]\ndestination = \"slack\"\ntemplate = \"herald:nudge\"\nprivate = false\n"
    ));
    let script = Script::answering(&[
        (
            GRAPHQL,
            200,
            &page("Change", "PRIVATE", "2026-10-30T11:00:00Z"),
        ),
        POSTED,
    ]);
    let swept = round(&config, &healthy(), &script, NOON);
    assert_eq!(announced(&swept[0])[0].1, [("nudges".to_owned(), true)]);
}

#[test]
fn a_rule_whose_view_cannot_be_read_or_whose_nudge_makes_no_event_stops_until_the_next_round() {
    let config = config(RULE);
    let store = healthy();
    let script = Script::answering(&[(GRAPHQL, 502, "")]);
    let swept = round(&config, &store, &script, NOON);
    assert!(
        matches!(&swept[0].outcome, Sweep::Stopped(Stop::Source(_))),
        "{swept:?}"
    );
    let untitled =
        Script::answering(&[(GRAPHQL, 200, &page(" ", "PUBLIC", "2026-10-30T11:00:00Z"))]);
    let swept = round(&config, &store, &untitled, NOON);
    assert!(
        matches!(&swept[0].outcome, Sweep::Stopped(Stop::Source(_))),
        "{swept:?}"
    );
    let failing = Failing {
        fault: crate::harness::Fault::Cursors,
        ..healthy()
    };
    let swept = round(&config, &failing, &Script::default(), NOON);
    assert!(
        matches!(&swept[0].outcome, Sweep::Stopped(Stop::Store(_))),
        "{swept:?}"
    );
    let stopped = Script::answering(&[
        (
            GRAPHQL,
            200,
            &page("Change", "PUBLIC", "2026-10-30T11:00:00Z"),
        ),
        ("/channels/1566200000000000100/messages", 500, ""),
    ]);
    let swept = round(&config, &store, &stopped, NOON);
    let Sweep::Reported { cursor, .. } = &swept[0].outcome else {
        panic!("{swept:?}");
    };
    assert_eq!(*cursor, Remembered::Kept);
}
