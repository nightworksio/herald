//! A report's load, counted from the rows each route is shown: a route
//! shown only the public repositories' rows is shown no team, figure or
//! name of a private one's (ADR-0075).

use herald_config::{Config, Uses, load};
use herald_flows::{Connected, Ports, prepare, report};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use serde_json::json;

use crate::harness::{NOON, POSTED, Script, healthy};

const GRAPHQL: &str = "https://api.github.com/graphql";
const TEAM: (&str, u16, &str) = (
    "/channels/1566200000000000200/messages",
    200,
    r#"{"id":"2"}"#,
);

fn config() -> Config {
    let text = r#"version = 1
[store]
kind = "sqlite"
[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }
[sources.open]
kind = "github"
repository = "o/open"
token = { secret = "HERALD_GITHUB_TOKEN" }
[sources.closed]
kind = "github"
repository = "o/closed"
token = { secret = "HERALD_GITHUB_TOKEN" }
[teams."o/reviewers"]
members = ["ann", "ben", "cat"]
[teams."o/secret-team"]
members = ["dan", "eve", "fay"]
[views.queue]
load = {}
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
title = "{{ view.name }}"
body = "load:{% if view.overviews.load %}{% for team in view.overviews.load.teams %} {{ team.name }}={{ team.requested }}{% endfor %}{% endif %}"
"#;
    load(
        "herald.toml",
        text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

/// A GraphQL page of the repository `path`, seen as `visibility`, with one
/// open pull request whose review is asked of `asked`, each a login or a
/// team's slug.
fn page(path: &str, visibility: &str, asked: &[&str]) -> String {
    let nodes: Vec<serde_json::Value> = asked
        .iter()
        .map(|asked| {
            if asked.contains('/') {
                json!({ "requestedReviewer": { "combinedSlug": asked } })
            } else {
                json!({ "requestedReviewer": { "login": asked } })
            }
        })
        .collect();
    json!({ "data": { "repository": { "visibility": visibility, "pullRequests": { "nodes": [ {
        "number": 1, "title": "Change", "url": format!("https://github.com/{path}/pull/1"),
        "isDraft": false, "createdAt": "2026-10-30T10:00:00Z", "updatedAt": "2026-10-30T11:00:00Z",
        "reviewRequests": { "totalCount": nodes.len(), "nodes": nodes }
    } ] } } } })
    .to_string()
}

#[test]
fn a_public_route_is_shown_the_load_of_the_public_rows_alone() {
    let config = config();
    let store = healthy();
    let script = Script::answering(&[
        (
            GRAPHQL,
            200,
            &page("o/closed", "PRIVATE", &["o/secret-team", "ann"]),
        ),
        (GRAPHQL, 200, &page("o/open", "PUBLIC", &["ann"])),
        TEAM,
        POSTED,
    ]);
    let secrets = FakeSecrets::default()
        .with("HERALD_DISCORD_TOKEN", "hunter2")
        .with("HERALD_GITHUB_TOKEN", "ghp_x");
    let prepared =
        pollster::block_on(prepare(&config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    let swept = pollster::block_on(report(&config, &connected, &ports, "r1"));
    assert_eq!(swept.len(), 1, "{swept:?}");
    let bodies: Vec<String> = script
        .seen()
        .into_iter()
        .zip(script.bodies())
        .filter(|(line, _)| line.contains("/channels/"))
        .map(|(_, body)| {
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            body["embeds"][0]["description"]
                .as_str()
                .unwrap_or_default()
                .replace('\\', "")
        })
        .collect();
    assert_eq!(bodies.len(), 2, "{bodies:?}");
    assert_eq!(
        bodies,
        ["load: o/reviewers=2 o/secret-team=1", "load: o/reviewers=1"]
    );
}
