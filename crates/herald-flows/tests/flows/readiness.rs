//! A report's release verdict, judged from the rows each route is shown:
//! a route shown only the public repositories' rows is told nothing of a
//! private one's pull request holding the release back (ADR-0081).

use herald_config::{Uses, load};
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

const TEXT: &str = r#"version = 1
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
[views.queue]
release = { milestone = "v2.*" }
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
body = "{{ view.total }}{% if view.readiness %} {% if view.readiness.ready %}ready{% else %}not ready:{% for hold in view.readiness.holds %} {{ hold.repository }}:{{ hold.number }}={{ hold.reason }}{% endfor %}{% endif %}{% endif %}"
"#;

/// A GraphQL page of the repository `path`, seen as `visibility`, with an
/// open pull request in milestone v2.0, a draft where `draft` says, and
/// one in no milestone that is a draft.
fn page(path: &str, visibility: &str, draft: bool) -> String {
    let node = |number: u64, draft: bool, milestone: Option<&str>| {
        json!({
            "number": number, "title": "Change", "url": format!("https://github.com/{path}/pull/{number}"),
            "isDraft": draft, "createdAt": "2026-10-30T10:00:00Z", "updatedAt": "2026-10-30T11:00:00Z",
            "milestone": milestone.map(|title| json!({ "title": title })),
        })
    };
    json!({ "data": { "repository": { "visibility": visibility, "pullRequests": { "nodes": [
        node(1, draft, Some("v2.0")),
        node(2, true, None),
    ] } } } })
    .to_string()
}

#[test]
fn a_public_route_is_judged_by_the_public_rows_alone() {
    let config = load(
        "herald.toml",
        TEXT,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let store = healthy();
    let script = Script::answering(&[
        (GRAPHQL, 200, &page("o/closed", "PRIVATE", true)),
        (GRAPHQL, 200, &page("o/open", "PUBLIC", false)),
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
    assert_eq!(bodies, ["2 not ready: o/closed:1=draft", "1 ready"]);
}
