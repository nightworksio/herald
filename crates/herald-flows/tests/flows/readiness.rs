//! A report's release verdict on each route: a route shown only the
//! public repositories' rows is told the verdict is not known, judged by
//! those rows, or the whole verdict with a private hold named private, as
//! the release's `withheld` says, and nothing more of a private pull
//! request (ADR-0081).

use herald_config::{Uses, load};
use herald_core::{Held, Printed, Showing, view_json};
use herald_flows::{Connected, Ports, prepare, queue, report};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use serde_json::json;

use crate::harness::{NOON, POSTED, Script, healthy};

const GRAPHQL: &str = "https://api.github.com/graphql";
const CLOSED_MILESTONES: (&str, u16, &str) = (
    "https://api.github.com/repos/o/closed/milestones?state=open&per_page=30",
    200,
    "[]",
);
const OPEN_MILESTONES: (&str, u16, &str) = (
    "https://api.github.com/repos/o/open/milestones?state=open&per_page=30",
    200,
    "[]",
);
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
release = { milestone = "v2.*"WITHHELD }
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
body = "{{ view.total }}{% if view.readiness %} {% if view.readiness.ready is none %}unknown{% elif view.readiness.ready %}ready{% else %}not ready:{% for hold in view.readiness.holds %} {% if hold.private %}private{% else %}{{ hold.repository }}:{{ hold.number }}={{ hold.reason }}{% endif %}{% endfor %}{% endif %}{% endif %}"
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

/// What the private route and then the public route are told where the
/// release says `withheld`, the private repository's pull request is a
/// draft where `draft` says and the public one's where `open` says.
fn told(withheld: &str, draft: bool) -> Vec<String> {
    told_beside(withheld, draft, false)
}

fn told_beside(withheld: &str, draft: bool, open: bool) -> Vec<String> {
    let config = load(
        "herald.toml",
        &TEXT.replace("WITHHELD", withheld),
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let store = healthy();
    let script = Script::answering(&[
        (GRAPHQL, 200, &page("o/closed", "PRIVATE", draft)),
        CLOSED_MILESTONES,
        (GRAPHQL, 200, &page("o/open", "PUBLIC", open)),
        OPEN_MILESTONES,
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
    script
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
        .collect()
}

#[test]
fn a_public_route_is_told_the_verdict_is_not_known_by_default() {
    for withheld in ["", ", withheld = \"unknown\""] {
        assert_eq!(
            told(withheld, true),
            ["2 not ready: o/closed:1=draft", "1 unknown"]
        );
        assert_eq!(told(withheld, false), ["2 ready", "1 unknown"]);
    }
}

#[test]
fn a_public_route_a_shown_pull_request_holds_is_told_not_ready_and_nothing_of_a_private_one() {
    for draft in [true, false] {
        let told = told_beside("", draft, true);
        assert_eq!(told[1], "1 not ready: o/open:1=draft");
    }
}

#[test]
fn a_public_route_judged_by_what_it_is_shown_is_told_ready() {
    let shown = ", withheld = \"shown\"";
    assert_eq!(
        told(shown, true),
        ["2 not ready: o/closed:1=draft", "1 ready"]
    );
}

#[test]
fn a_masked_public_route_is_told_the_whole_verdict_with_the_private_hold_named_private() {
    let masked = ", withheld = \"masked\"";
    assert_eq!(
        told(masked, true),
        ["2 not ready: o/closed:1=draft", "1 not ready: private"]
    );
    assert_eq!(told(masked, false), ["2 ready", "1 ready"]);
}

/// The board `herald queue` prints of the view without `--private`, the
/// private repository's pull request a draft where `draft` says and the
/// public one's where `open` says.
fn queued(draft: bool, open: bool) -> herald_core::Board {
    let config = load(
        "herald.toml",
        &TEXT.replace("WITHHELD", ""),
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let store = healthy();
    let script = Script::answering(&[
        (GRAPHQL, 200, &page("o/closed", "PRIVATE", draft)),
        CLOSED_MILESTONES,
        (GRAPHQL, 200, &page("o/open", "PUBLIC", open)),
        OPEN_MILESTONES,
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
    pollster::block_on(queue(
        &config,
        &connected,
        &ports,
        ("queue", Printed::Withheld),
    ))
    .unwrap()
}

#[test]
fn herald_queue_unprivate_is_told_not_ready_by_a_printed_hold_and_not_known_without_one() {
    for draft in [true, false] {
        let held = queued(draft, true);
        let readiness = held.readiness.clone().unwrap();
        assert_eq!(readiness.ready(), Some(false));
        let [Held::Shown(holding)] = readiness.holds.as_slice() else {
            panic!("one shown hold: {readiness:?}");
        };
        assert_eq!(holding.repository, "o/open");
        assert!(matches!(
            holding.holder,
            herald_core::Holder::Pull { number: 1, .. }
        ));
        let json = view_json(Showing {
            view: "queue",
            board: &held,
        });
        assert!(!json.contains("o/closed"), "{json}");
        let read: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(read["readiness"]["ready"], false);
        let clear = queued(draft, false);
        assert_eq!(clear.readiness.unwrap().ready(), None);
    }
}
