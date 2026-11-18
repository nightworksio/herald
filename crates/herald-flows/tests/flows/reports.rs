//! A report's window announced once through the routes naming it: its
//! view read from every source it names, every row to a private route,
//! and only the public repositories' rows to the rest while
//! `[doctor] private_to_public` refuses (ADR-0055).

use herald_config::{Config, Uses, load};
use herald_core::SourceName;
use herald_flows::{Connected, Outcome, Ports, Remembered, Stop, Sweep, Swept, prepare, report};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use serde_json::json;

use crate::harness::{Failing, NOON, POSTED, Script, healthy};

const GRAPHQL: &str = "https://api.github.com/graphql";
const TEAM: (&str, u16, &str) = (
    "/channels/1566200000000000200/messages",
    200,
    r#"{"id":"2"}"#,
);

fn config(doctor: &str) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[sources.open]
kind = "github"
repository = "o/open"
token = {{ secret = "HERALD_GITHUB_TOKEN" }}
[sources.closed]
kind = "github"
repository = "o/closed"
token = {{ secret = "HERALD_GITHUB_TOKEN" }}
[reports.daily]
[run]
routes = 1
[[routes]]
id = "public"
sources = ["daily"]
destination = "discord"
channel = "1566200000000000100"
template = "t"
private = false
[[routes]]
id = "team"
sources = ["daily"]
destination = "discord"
channel = "1566200000000000200"
template = "t"
private = true
[templates.t]
title = "{{{{ view.name }}}}"
body = "rows:{{% for group in view.groups %}}{{% for row in group.rows %}} {{{{ row.repository }}}}{{% endfor %}}{{% endfor %}}"
{doctor}
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

/// A GraphQL page of the repository `path`, seen as `visibility`, with one
/// open pull request.
fn page(path: &str, visibility: &str) -> String {
    json!({ "data": { "repository": { "visibility": visibility, "pullRequests": { "nodes": [ {
        "number": 1, "title": "Change", "url": format!("https://github.com/{path}/pull/1"),
        "isDraft": false, "createdAt": "2026-10-30T10:00:00Z", "updatedAt": "2026-10-30T11:00:00Z"
    } ] } } } })
    .to_string()
}

fn reported(config: &Config, store: &Failing, script: &Script) -> Vec<Swept> {
    let secrets = FakeSecrets::default()
        .with("HERALD_DISCORD_TOKEN", "hunter2")
        .with("HERALD_GITHUB_TOKEN", "ghp_x");
    let prepared = pollster::block_on(prepare(config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store,
        clock: &clock,
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    pollster::block_on(report(config, &connected, &ports, "r1"))
}

/// What each route posted, by its channel, in order.
fn posted(script: &Script) -> Vec<(String, String)> {
    script
        .seen()
        .into_iter()
        .zip(script.bodies())
        .filter(|(line, _)| line.contains("/channels/"))
        .map(|(line, body)| {
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let channel = line.split('/').nth(5).unwrap_or_default().to_owned();
            (channel, body.to_string())
        })
        .collect()
}

fn routes_of(sweep: &Sweep) -> Vec<Vec<(String, bool)>> {
    let Sweep::Reported { announced, .. } = sweep else {
        panic!("{sweep:?}");
    };
    announced
        .iter()
        .map(|fresh| {
            fresh
                .announced
                .iter()
                .map(|announced| {
                    (
                        announced.route.as_str().to_owned(),
                        matches!(announced.outcome, Outcome::Sent { .. }),
                    )
                })
                .collect()
        })
        .collect()
}

#[test]
fn a_mixed_view_shows_a_public_route_only_the_public_rows_and_is_reported_once() {
    let config = config("");
    let store = healthy();
    let script = Script::answering(&[
        (GRAPHQL, 200, &page("o/closed", "PRIVATE")),
        (GRAPHQL, 200, &page("o/open", "PUBLIC")),
        TEAM,
        POSTED,
    ]);
    let swept = reported(&config, &store, &script);
    assert_eq!(swept.len(), 1);
    assert_eq!(swept[0].source, SourceName::new("daily"));
    let Sweep::Reported {
        pulls,
        announced,
        cursor,
    } = &swept[0].outcome
    else {
        panic!("{swept:?}");
    };
    assert_eq!(*pulls, 2);
    assert_eq!(*cursor, Remembered::Advanced);
    assert_eq!(
        announced[0].event.identity().as_str(),
        "report/daily/2026-10-31T09:00:00Z"
    );
    assert_eq!(
        routes_of(&swept[0].outcome),
        [
            vec![("team".to_owned(), true)],
            vec![("public".to_owned(), true)]
        ]
    );
    let posts = posted(&script);
    assert_eq!(posts.len(), 2);
    assert!(
        posts[0].1.contains("closed") && posts[0].1.contains("open"),
        "{posts:?}"
    );
    assert!(
        posts[1].1.contains("open") && !posts[1].1.contains("closed"),
        "{posts:?}"
    );
    let again = Script::default();
    assert_eq!(reported(&config, &store, &again), []);
    assert_eq!(again.seen(), Vec::<String>::new());
}

#[test]
fn an_all_private_view_never_reaches_a_public_route() {
    let config = config("");
    let script = Script::answering(&[
        (GRAPHQL, 200, &page("o/closed", "PRIVATE")),
        (GRAPHQL, 200, &page("o/open", "INTERNAL")),
        TEAM,
    ]);
    let swept = reported(&config, &healthy(), &script);
    assert_eq!(
        routes_of(&swept[0].outcome),
        [vec![("team".to_owned(), true)]]
    );
    assert_eq!(posted(&script).len(), 1);
}

#[test]
fn a_view_allowed_to_go_public_shows_every_route_every_row() {
    let config = config("[doctor]\nprivate_to_public = \"allow\"");
    let script = Script::answering(&[
        (GRAPHQL, 200, &page("o/closed", "PRIVATE")),
        (GRAPHQL, 200, &page("o/open", "PUBLIC")),
        POSTED,
        TEAM,
    ]);
    let swept = reported(&config, &healthy(), &script);
    assert_eq!(
        routes_of(&swept[0].outcome),
        [vec![("public".to_owned(), true), ("team".to_owned(), true)]]
    );
    let posts = posted(&script);
    assert!(
        posts.iter().all(|(_, body)| body.contains("closed")),
        "{posts:?}"
    );
}

#[test]
fn a_source_that_cannot_be_read_stops_the_report_until_the_next_round() {
    let config = config("");
    let store = healthy();
    let script = Script::answering(&[(GRAPHQL, 502, "")]);
    let swept = reported(&config, &store, &script);
    assert!(
        matches!(&swept[0].outcome, Sweep::Stopped(Stop::Source(_))),
        "{swept:?}"
    );
    let script = Script::answering(&[
        (GRAPHQL, 200, &page("o/closed", "PUBLIC")),
        (GRAPHQL, 200, &page("o/open", "PUBLIC")),
        POSTED,
        TEAM,
    ]);
    let swept = reported(&config, &store, &script);
    assert!(
        matches!(swept[0].outcome, Sweep::Reported { .. }),
        "{swept:?}"
    );
}
