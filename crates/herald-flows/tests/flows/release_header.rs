//! A release's header on each surface: the open milestones its selector
//! matches in each repository it shows a pull request of the release in,
//! a public route and `herald queue` without `--private` told those of
//! public repositories alone, each repository read once and at most
//! twenty, one that could not be read named with why (ADR-0082); and the
//! open issues of those milestones as rows, seen the same way (ADR-0084).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use herald_config::{Uses, load};
use herald_core::{Board, Listed, Listing, Membership, Owner, Printed, SourceName, Visibility};
use herald_flows::{Connected, Ports, prepare, queue, report};
use herald_ports::Cursors as _;
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use serde_json::json;

use crate::harness::{Failing, NOON, POSTED, Script, healthy};

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
SOURCES
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
body = "{{ view.total }}{% if view.overviews.header %}{% for m in view.overviews.header.milestones %} {{ m.repository }}:{{ m.title }}{% endfor %}{% for u in view.overviews.header.unread %} unread:{{ u.repository }}{% endfor %}{% endif %}{% if view.issues %}{% for i in view.issues %} {{ i.repository }}#{{ i.number }}{% endfor %}{% endif %}"
"#;

fn sources(paths: &[&str]) -> String {
    let mut text = String::new();
    for path in paths {
        let name = path.replace('/', "-");
        let _ = writeln!(
            text,
            "[sources.{name}]\nkind = \"github\"\nrepository = \"{path}\"\ntoken = {{ secret = \"HERALD_GITHUB_TOKEN\" }}"
        );
    }
    text
}

/// A GraphQL page of the repository `path`, seen as `visibility`, with an
/// open pull request in milestone v2.0 where `picked` says.
fn page(path: &str, visibility: &str, picked: bool) -> String {
    let nodes: Vec<serde_json::Value> = [picked]
        .into_iter()
        .filter(|picked| *picked)
        .map(|_| {
            json!({
                "number": 1, "title": "Change", "url": format!("https://github.com/{path}/pull/1"),
                "isDraft": false, "createdAt": "2026-10-30T10:00:00Z",
                "updatedAt": "2026-10-30T11:00:00Z", "milestone": { "title": "v2.0" },
            })
        })
        .collect();
    json!({ "data": { "repository": { "visibility": visibility, "pullRequests": { "nodes": nodes } } } })
        .to_string()
}

fn milestones_url(path: &str) -> String {
    format!("https://api.github.com/repos/{path}/milestones?state=open&per_page=30")
}

fn milestones(path: &str) -> String {
    json!([{
        "id": 1, "number": 1, "title": "v2.0", "description": null, "state": "open",
        "open_issues": 1, "closed_issues": 3, "created_at": "2026-10-01T08:00:00Z",
        "updated_at": "2026-10-02T08:00:00Z", "closed_at": null, "due_on": "2026-10-01T08:00:00Z",
        "html_url": format!("https://github.com/{path}/milestone/1"),
    }, {
        "id": 2, "number": 2, "title": "v3.0", "description": null, "state": "open",
        "open_issues": 0, "closed_issues": 0, "created_at": "2026-10-01T08:00:00Z",
        "updated_at": "2026-10-02T08:00:00Z", "closed_at": null, "due_on": null,
        "html_url": format!("https://github.com/{path}/milestone/2"),
    }])
    .to_string()
}

fn connected(paths: &[&str], script: &Script) -> (herald_config::Config, Connected<Script>) {
    connected_as(TEXT, paths, script)
}

fn connected_as(
    text: &str,
    paths: &[&str],
    script: &Script,
) -> (herald_config::Config, Connected<Script>) {
    let config = load(
        "herald.toml",
        &text.replace("SOURCES", &sources(paths)),
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let secrets = FakeSecrets::default()
        .with("HERALD_DISCORD_TOKEN", "hunter2")
        .with("HERALD_GITHUB_TOKEN", "ghp_x");
    let prepared =
        pollster::block_on(prepare(&config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    (config, connected)
}

fn queued(paths: &[&str], script: &Script, printed: Printed) -> Board {
    queued_as(TEXT, paths, script, printed)
}

fn queued_as(text: &str, paths: &[&str], script: &Script, printed: Printed) -> Board {
    let (config, connected) = connected_as(text, paths, script);
    let store = healthy();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    pollster::block_on(queue(&config, &connected, &ports, ("queue", printed))).unwrap()
}

/// What the private route and then the public route are told of the
/// sources `paths`, each read by its `answers`.
fn told(paths: &[&str], answers: &[(&str, u16, &str)]) -> Vec<String> {
    told_as(TEXT, paths, answers)
}

fn told_as(text: &str, paths: &[&str], answers: &[(&str, u16, &str)]) -> Vec<String> {
    told_in(text, paths, answers, &healthy())
}

/// What the private route and then the public route are told of the
/// sources `paths`, each read by its `answers`, `store` keeping what it
/// keeps.
fn told_in(
    text: &str,
    paths: &[&str],
    answers: &[(&str, u16, &str)],
    store: &Failing,
) -> Vec<String> {
    let script = Script::answering(answers);
    let (config, connected) = connected_as(text, paths, &script);
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store,
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
                .replace(['\\', '\u{200b}'], "")
        })
        .collect()
}

#[test]
fn a_public_route_is_told_the_milestones_of_public_repositories_alone() {
    let (closed, open) = (milestones_url("o/closed"), milestones_url("o/open"));
    let told = told(
        &["o/closed", "o/open"],
        &[
            (GRAPHQL, 200, &page("o/closed", "PRIVATE", true)),
            (GRAPHQL, 200, &page("o/open", "PUBLIC", true)),
            (&open, 200, &milestones("o/open")),
            (&closed, 200, &milestones("o/closed")),
            TEAM,
            POSTED,
        ],
    );
    assert_eq!(told, ["2 o/open:v2.0 o/closed:v2.0", "1 o/open:v2.0"]);
}

#[test]
fn a_milestone_seen_as_private_alone_makes_a_private_audience_of_its_own() {
    let unpicked = page("o/open", "PUBLIC", true).replace("v2.0", "v9.0");
    let open = milestones_url("o/open");
    let told = told(
        &["o/open"],
        &[
            (GRAPHQL, 200, &unpicked),
            (&open, 200, &milestones("o/open")),
            TEAM,
            POSTED,
        ],
    );
    assert_eq!(told, ["0 o/open:v2.0"]);
}

#[test]
fn herald_queue_prints_the_milestones_of_the_repositories_it_prints_alone() {
    let script = || {
        Script::answering(&[
            (GRAPHQL, 200, &page("o/closed", "PRIVATE", true)),
            (GRAPHQL, 200, &page("o/open", "PUBLIC", true)),
            (&milestones_url("o/open"), 404, "{}"),
            (&milestones_url("o/closed"), 200, &milestones("o/closed")),
        ])
    };
    let withheld = queued(&["o/closed", "o/open"], &script(), Printed::Withheld);
    let header = withheld.overviews.header.unwrap();
    assert!(header.milestones.is_empty(), "{header:?}");
    assert_eq!(header.unread.len(), 1, "{header:?}");
    assert_eq!(header.unread[0].repository, "o/open");
    let asked = queued(&["o/closed", "o/open"], &script(), Printed::Asked);
    let header = asked.overviews.header.unwrap();
    let milestone = &header.milestones[0];
    assert_eq!(
        (milestone.repository.as_str(), milestone.title.as_str()),
        ("o/closed", "v2.0")
    );
    assert_eq!(milestone.counts, Some((1, 3)));
    assert!(milestone.overdue);
    assert_eq!(header.milestones.len(), 1);
}

#[test]
fn a_repository_whose_milestones_could_not_be_read_is_named_with_why() {
    let script = Script::answering(&[
        (GRAPHQL, 200, &page("o/open", "PRIVATE", true)),
        (&milestones_url("o/open"), 429, "{}"),
    ]);
    let board = queued(&["o/open"], &script, Printed::Asked);
    let header = board.overviews.header.unwrap();
    assert_eq!(header.unread[0].visibility, Visibility::Private);
    assert!(
        header.unread[0]
            .reason
            .starts_with("its milestones could not be read: "),
        "{header:?}"
    );
}

#[test]
fn a_source_s_repository_with_no_pull_request_is_read_as_private_and_once() {
    let script = || {
        Script::answering(&[
            (GRAPHQL, 200, &page("o/open", "PUBLIC", false)),
            (&milestones_url("o/open"), 200, &milestones("o/open")),
        ])
    };
    let withheld = queued(&["o/open"], &script(), Printed::Withheld);
    assert_eq!(
        withheld.overviews.header.unwrap(),
        herald_core::Header::default()
    );
    let shown = script();
    let asked = queued(&["o/open"], &shown, Printed::Asked);
    assert_eq!(asked.overviews.header.unwrap().milestones.len(), 1);
    assert_eq!(shown.seen().len(), 2, "{:?}", shown.seen());
}

#[test]
fn a_header_reads_the_milestones_of_at_most_twenty_repositories() {
    for (count, unread) in [(20, 0), (21, 1)] {
        let paths: Vec<String> = (0..count).map(|index| format!("o/r{index:02}")).collect();
        let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
        let pages: Vec<(String, String)> = paths
            .iter()
            .map(|path| (page(path, "PUBLIC", true), milestones_url(path)))
            .collect();
        let mut answers: Vec<(&str, u16, &str)> = pages
            .iter()
            .map(|(page, _)| (GRAPHQL, 200, page.as_str()))
            .collect();
        answers.extend(
            pages
                .iter()
                .take(20)
                .map(|(_, url)| (url.as_str(), 200, "[]")),
        );
        let script = Script::answering(&answers);
        let board = queued(&paths, &script, Printed::Withheld);
        let header = board.overviews.header.unwrap();
        assert_eq!(header.unread.len(), unread, "{count}: {header:?}");
        if unread == 1 {
            assert_eq!(header.unread[0].repository, "o/r20");
            assert_eq!(
                header.unread[0].reason,
                "a release's header reads the milestones of at most 20 repositories and groups"
            );
        }
    }
}

#[test]
fn a_release_with_no_header_reads_no_milestone() {
    let script = Script::answering(&[(GRAPHQL, 200, &page("o/open", "PUBLIC", true))]);
    let (config, connected) = connected(&["o/open"], &script);
    let mut config = config;
    for view in config.views.values_mut() {
        if let Some(release) = view.view.release.as_mut() {
            release.header = false;
        }
    }
    let store = healthy();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    let board = pollster::block_on(queue(
        &config,
        &connected,
        &ports,
        ("queue", Printed::Asked),
    ))
    .unwrap();
    assert_eq!(board.overviews.header, None);
    assert_eq!(script.seen().len(), 1, "{:?}", script.seen());
}

#[test]
fn a_repository_the_view_does_not_hold_or_with_no_pull_request_of_the_release_is_not_seen_as_its_rows()
 {
    let text = TEXT
        .replace("SOURCES", &sources(&["o/closed", "o/open"]))
        .replace("release = {", "repositories = [\"o/open\"]\nrelease = {");
    let config = load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let unpicked = page("o/open", "PUBLIC", true).replace("v2.0", "v9.0");
    let script = Script::answering(&[
        (GRAPHQL, 200, &page("o/closed", "PRIVATE", true)),
        (GRAPHQL, 200, &unpicked),
        (&milestones_url("o/open"), 200, &milestones("o/open")),
    ]);
    let secrets = FakeSecrets::default()
        .with("HERALD_DISCORD_TOKEN", "hunter2")
        .with("HERALD_GITHUB_TOKEN", "ghp_x");
    let prepared =
        pollster::block_on(prepare(&config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let store = healthy();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    let board = pollster::block_on(queue(
        &config,
        &connected,
        &ports,
        ("queue", Printed::Withheld),
    ))
    .unwrap();
    assert_eq!(
        board.overviews.header.unwrap(),
        herald_core::Header::default()
    );
    assert_eq!(script.seen().len(), 3, "{:?}", script.seen());
}

fn issues_url(path: &str) -> String {
    format!("https://api.github.com/repos/{path}/issues?milestone=1&state=open&per_page=30")
}

fn issues(path: &str, number: u64) -> String {
    json!([{
        "number": number, "title": "Crash", "state": "open",
        "html_url": format!("https://github.com/{path}/issues/{number}"),
        "user": { "login": "ann", "type": "User" }, "state_reason": null,
        "created_at": "2026-10-01T08:00:00Z", "updated_at": null, "closed_at": null,
    }])
    .to_string()
}

#[test]
fn a_public_route_is_told_the_issue_rows_of_public_repositories_alone() {
    let text = TEXT.replace(
        "release = { milestone = \"v2.*\" }",
        "release = { milestone = \"v2.*\", rows = \"issues\", header = false }",
    );
    let told = told_as(
        &text,
        &["o/closed", "o/open"],
        &[
            (GRAPHQL, 200, &page("o/closed", "PRIVATE", true)),
            (GRAPHQL, 200, &page("o/open", "PUBLIC", true)),
            (&milestones_url("o/open"), 200, &milestones("o/open")),
            (&issues_url("o/open"), 200, &issues("o/open", 8)),
            (&milestones_url("o/closed"), 200, &milestones("o/closed")),
            (&issues_url("o/closed"), 200, &issues("o/closed", 7)),
            TEAM,
            POSTED,
        ],
    );
    assert_eq!(told, ["2 o/open#8 o/closed#7", "1 o/open#8"]);
}

#[test]
fn a_release_listing_issues_whose_milestones_could_not_be_read_lists_none_and_says_why() {
    let text = TEXT.replace(
        "release = { milestone = \"v2.*\" }",
        "release = { milestone = \"v2.*\", rows = \"issues\" }",
    );
    let script = Script::answering(&[
        (GRAPHQL, 200, &page("o/open", "PUBLIC", true)),
        (&milestones_url("o/open"), 500, "{}"),
    ]);
    let board = queued_as(&text, &["o/open"], &script, Printed::Asked);
    let header = board.overviews.header.unwrap();
    assert_eq!(header.issues, Some(Vec::new()));
    assert_eq!(header.unread.len(), 1, "{header:?}");
    assert_eq!(header.unread[0].repository, "o/open");
}

const SEARCH: &str =
    "/search/issues?q=is%3Apr%20is%3Aopen%20org%3Ao&sort=updated&order=desc&per_page=100&page=1";

/// A store holding the organization `o`'s listing, `members` its members.
fn owned(members: &[(&str, Visibility)]) -> Failing {
    let store = healthy();
    let source = SourceName::new("org");
    let listing = Listing {
        owner: Owner::Organization,
        pages: Vec::new(),
        first: NOON,
        listed: NOON,
        fingerprint: String::new(),
        file: None,
        events: None,
        stale: false,
    };
    pollster::block_on(store.put_listing(&source, &listing, None)).unwrap();
    for (id, &(name, visibility)) in (1..).zip(members) {
        let repository = Listed {
            id: id.to_string(),
            name: name.to_owned(),
            path: format!("o/{name}"),
            topics: Vec::new(),
            properties: BTreeMap::new(),
            visibility,
            archived: false,
            fork: false,
            template: false,
            language: None,
            linked: None,
        };
        let membership = Membership {
            repository,
            discovered: NOON,
            left: None,
            cursor: None,
            collides: None,
            hooked: false,
        };
        pollster::block_on(store.put_member(&source, &membership, None)).unwrap();
    }
    store
}

/// The search finding a pull request in milestone v2.0 in each of
/// `names`, then GraphQL reading them.
fn searched(names: &[&str]) -> (String, String) {
    let items: Vec<serde_json::Value> = (1..)
        .zip(names)
        .map(|(number, name)| {
            json!({ "number": number, "title": "t",
                "html_url": format!("https://github.com/o/{name}/pull/{number}"),
                "created_at": "2026-10-30T10:00:00Z", "updated_at": "2026-10-30T11:00:00Z",
                "repository_url": format!("https://api.github.com/repos/o/{name}") })
        })
        .collect();
    let nodes: serde_json::Map<String, serde_json::Value> = (1..)
        .zip(names)
        .enumerate()
        .map(|(index, (number, name))| {
            let node = json!({ "number": number, "title": format!("read {number}"),
                "url": format!("https://github.com/o/{name}/pull/{number}"), "isDraft": false,
                "createdAt": "2026-10-30T10:00:00Z", "updatedAt": "2026-10-30T11:00:00Z",
                "headRefOid": format!("h{number}"), "milestone": { "title": "v2.0" } });
            (
                format!("r{index}"),
                json!({ "visibility": "PUBLIC", "p0": node }),
            )
        })
        .collect();
    (
        json!({ "total_count": names.len(), "items": items }).to_string(),
        json!({ "data": nodes }).to_string(),
    )
}

#[test]
fn a_public_route_is_told_nothing_of_an_owner_s_private_or_internal_members_under_each_withheld_mode()
 {
    let owner = "[sources.org]\nkind = \"github\"\nowner = \"o\"\ntoken = { secret = \"HERALD_GITHUB_TOKEN\" }\nvisibility = [\"public\", \"internal\", \"private\"]";
    let (app, vault) = (milestones_url("o/app"), milestones_url("o/vault"));
    let inside = milestones_url("o/inside");
    let secret = milestones("o/vault").replace("v2.0", "v2.7-vault");
    for withheld in ["unknown", "shown", "masked"] {
        let text = TEXT.replace("SOURCES", owner).replace(
            "release = { milestone = \"v2.*\" }",
            &format!(
                "mode = \"live\"\nrelease = {{ milestone = \"v2.*\", withheld = \"{withheld}\" }}"
            ),
        );
        let (found, read) = searched(&["app", "vault"]);
        let spanned = told_in(
            &text,
            &[],
            &[
                (SEARCH, 200, &found),
                (GRAPHQL, 200, &read),
                (&app, 200, &milestones("o/app")),
                (&inside, 403, "{}"),
                (&vault, 200, &secret),
                TEAM,
                POSTED,
            ],
            &owned(&[
                ("app", Visibility::Public),
                ("vault", Visibility::Private),
                ("inside", Visibility::Internal),
            ]),
        );
        let (found, read) = searched(&["app"]);
        let alone = told_in(
            &text,
            &[],
            &[
                (SEARCH, 200, &found),
                (GRAPHQL, 200, &read),
                (&app, 200, &milestones("o/app")),
                POSTED,
                TEAM,
            ],
            &owned(&[("app", Visibility::Public)]),
        );
        assert_eq!(
            spanned[0], "2 o/app:v2.0 o/vault:v2.7-vault unread:o/inside",
            "{withheld}"
        );
        assert_eq!(alone, [spanned[1].as_str(), "1 o/app:v2.0"], "{withheld}");
    }
}

#[test]
fn a_gitlab_owner_s_group_milestones_are_private_and_its_issues_kept_where_the_view_holds_them() {
    let owner = "[sources.org]\nkind = \"gitlab\"\nowner = \"o\"\ntoken = { secret = \"HERALD_GITHUB_TOKEN\" }\nsignals = [\"milestone\"]\nmilestones = { group = true }";
    let text = TEXT.replace("SOURCES", owner).replace(
        "release = { milestone = \"v2.*\" }",
        "mode = \"live\"\nrepositories = [\"o/api\", \"o/vault\"]\nrelease = { milestone = \"v2.*\", rows = \"issues\", header = false }",
    );
    let issue = |full: &str, number: u64| {
        json!({
            "iid": number, "title": "Crash", "state": "opened",
            "web_url": format!("https://gitlab.com/{full}/-/issues/{number}"),
            "author": { "username": "ann" }, "created_at": "2026-10-01T08:00:00Z",
            "updated_at": null, "closed_at": null, "labels": [],
            "references": { "full": format!("{full}#{number}") },
        })
    };
    let milestone = json!([{
        "id": 1, "iid": 1, "title": "v2.0", "description": null, "state": "active",
        "group_id": 3, "due_date": null, "created_at": "2026-10-01T08:00:00Z",
        "updated_at": null, "web_url": "https://gitlab.com/groups/o/-/milestones/1",
    }])
    .to_string();
    let issues = json!([issue("o/api", 4), issue("o/vault", 5), issue("o/other", 6)]).to_string();
    let script = Script::answering(&[
        (
            "/groups/o/merge_requests?state=opened&scope=all&order_by=updated_at&sort=desc&per_page=100&page=1",
            200,
            "[]",
        ),
        (
            "/projects/o%2Fapi/milestones?state=active&per_page=30",
            200,
            "[]",
        ),
        (
            "/groups/o/milestones?state=active&per_page=30",
            200,
            &milestone,
        ),
        (
            "/groups/o/issues?milestone=v2.0&state=opened&per_page=30",
            200,
            &issues,
        ),
        (
            "/projects/o%2Fvault/milestones?state=active&per_page=30",
            200,
            "[]",
        ),
    ]);
    let (config, connected) = connected_as(&text, &[], &script);
    let store = owned(&[("api", Visibility::Public), ("vault", Visibility::Private)]);
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    let board = pollster::block_on(queue(
        &config,
        &connected,
        &ports,
        ("queue", Printed::Asked),
    ))
    .unwrap();
    let header = board.overviews.header.unwrap();
    let seen: Vec<(&str, Visibility)> = header
        .issues
        .iter()
        .flatten()
        .map(|issue| (issue.repository.as_str(), issue.visibility))
        .collect();
    assert_eq!(
        seen,
        [
            ("o/api", Visibility::Private),
            ("o/vault", Visibility::Private),
        ],
        "{:?}",
        script.seen()
    );
}
