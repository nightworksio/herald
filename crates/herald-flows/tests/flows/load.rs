//! A report's load, counted from the rows each route is shown: a route
//! shown only the public repositories' rows is shown no team, figure or
//! name of a private one's (ADR-0075), and a forge's teams reach only a
//! route that says private = true (ADR-0077).

use herald_config::{Config, Uses, load};
use herald_flows::{Connected, Ports, prepare, report};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use serde_json::json;

use crate::harness::{NOON, POSTED, Script, described, healthy};

const GRAPHQL: &str = "https://api.github.com/graphql";
const TEAM: (&str, u16, &str) = (
    "/channels/1566200000000000200/messages",
    200,
    r#"{"id":"2"}"#,
);

fn config() -> Config {
    loaded(TEXT)
}

fn loaded(text: &str) -> Config {
    load(
        "herald.toml",
        text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

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
    let bodies = described(&script);
    assert_eq!(bodies.len(), 2, "{bodies:?}");
    assert_eq!(
        bodies,
        ["load: o/reviewers=2 o/secret-team=1", "load: o/reviewers=1"]
    );
}

/// [`TEXT`] with each source reading its forge's teams, and no config team
/// named `o/secret-team`, its route `team` private where `private` says.
fn forge_teams(private: bool) -> Config {
    let text = TEXT
        .replace(
            "token = { secret = \"HERALD_GITHUB_TOKEN\" }\n",
            "token = { secret = \"HERALD_GITHUB_TOKEN\" }\nteams = \"forge\"\n",
        )
        .replace(
            "[teams.\"o/secret-team\"]\nmembers = [\"dan\", \"eve\", \"fay\"]\n",
            "",
        );
    let text = if private {
        text
    } else {
        text.replace("private = true", "private = false")
    };
    loaded(&text)
}

/// What a report of `config` posts, its forge answering `answers` after
/// the two repositories' pages; the requests it made.
fn reported(
    config: &Config,
    answers: &[(&str, u16, &str)],
    sent: &[(&str, u16, &str)],
) -> (Vec<String>, Vec<String>) {
    let (seen, bodies, _) = swept(config, answers, sent);
    (seen, bodies)
}

/// [`reported`], with what the round says of each report.
fn swept(
    config: &Config,
    answers: &[(&str, u16, &str)],
    sent: &[(&str, u16, &str)],
) -> (Vec<String>, Vec<String>, String) {
    let store = healthy();
    let closed = page("o/closed", "PRIVATE", &["o/secret-team", "ann"]);
    let open = page("o/open", "PUBLIC", &["ann", "dan"]);
    let script = Script::answering(&[(GRAPHQL, 200, &closed), (GRAPHQL, 200, &open)])
        .then(answers)
        .then(sent);
    let secrets = FakeSecrets::default()
        .with("HERALD_DISCORD_TOKEN", "hunter2")
        .with("HERALD_GITHUB_TOKEN", "ghp_x");
    let prepared = pollster::block_on(prepare(config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected: Connected<Script> = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store: &store,
        clock: &clock,
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    let swept = pollster::block_on(report(config, &connected, &ports, "r1"));
    (script.seen(), described(&script), format!("{swept:?}"))
}

const TEAMS: &str = "https://api.github.com/orgs/o/teams?per_page=100&page=1";

#[test]
fn a_forge_s_teams_reach_a_private_route_alone_read_once_an_owner_a_config_team_kept() {
    let answers = [
        (
            TEAMS,
            200,
            r#"[{"slug":"secret-team"},{"slug":"reviewers"}]"#,
        ),
        (
            "/orgs/o/teams/secret-team/members?per_page=100&page=1",
            200,
            r#"[{"login":"dan"},{"login":"eve"},{"login":"fay"}]"#,
        ),
        (
            "/orgs/o/teams/reviewers/members?per_page=100&page=1",
            200,
            r#"[{"login":"zed"}]"#,
        ),
    ];
    let (seen, bodies) = reported(&forge_teams(true), &answers, &[TEAM, POSTED]);
    assert_eq!(
        bodies,
        ["load: o/reviewers=2 o/secret-team=2", "load: o/reviewers=1"],
        "{seen:?}"
    );
    assert_eq!(seen.iter().filter(|line| line.ends_with(TEAMS)).count(), 1);
    let public = (
        "/channels/1566200000000000100/messages",
        200,
        r#"{"id":"3"}"#,
    );
    let (seen, bodies) = reported(&forge_teams(false), &[], &[public, TEAM]);
    assert_eq!(
        bodies,
        ["load: o/reviewers=1", "load: o/reviewers=1"],
        "{seen:?}"
    );
    assert!(!seen.iter().any(|line| line.contains("/teams")), "{seen:?}");
}

#[test]
fn a_forge_s_teams_unread_or_rate_limited_stop_the_report_and_refused_fold_as_config_teams_do() {
    let limited = r#"{"message":"You have exceeded a secondary rate limit."}"#;
    for (stopped, kind) in [
        ((TEAMS, crate::harness::UNCONNECTED, "down"), "Unreachable("),
        ((TEAMS, 403, limited), "RateLimited("),
        ((TEAMS, 429, "{}"), "RateLimited("),
    ] {
        let (seen, bodies, said) = swept(&forge_teams(true), &[stopped], &[]);
        assert_eq!(bodies, Vec::<String>::new(), "{seen:?}");
        assert!(said.contains(kind), "{said}");
    }
    let refused = [(TEAMS, 403, "{}")];
    let (seen, bodies) = reported(&forge_teams(true), &refused, &[TEAM, POSTED]);
    assert_eq!(
        bodies,
        ["load: o/reviewers=2", "load: o/reviewers=1"],
        "{seen:?}"
    );
}

#[test]
fn routes_shown_the_same_rows_are_parted_so_the_private_one_alone_is_shown_a_forge_s_teams() {
    let text = TEXT
        .replace(
            "[sources.open]\nkind = \"github\"\nrepository = \"o/open\"\ntoken = { secret = \"HERALD_GITHUB_TOKEN\" }\n",
            "[sources.open]\nkind = \"github\"\nrepository = \"o/open\"\ntoken = { secret = \"HERALD_GITHUB_TOKEN\" }\nteams = \"forge\"\n",
        )
        .replace("[views.queue]\n", "[views.queue]\nsources = [\"open\"]\n")
        .replace(
            "[teams.\"o/secret-team\"]\nmembers = [\"dan\", \"eve\", \"fay\"]\n",
            "",
        );
    assert!(!text.contains("o/secret-team") && text.contains("teams = \"forge\""));
    let config = loaded(&text);
    let store = healthy();
    let public = (
        "/channels/1566200000000000100/messages",
        200,
        r#"{"id":"3"}"#,
    );
    let script = Script::answering(&[
        (GRAPHQL, 200, &page("o/open", "PUBLIC", &["ann", "dan"])),
        (TEAMS, 200, r#"[{"slug":"secret-team"}]"#),
        (
            "/orgs/o/teams/secret-team/members?per_page=100&page=1",
            200,
            r#"[{"login":"dan"},{"login":"eve"},{"login":"fay"}]"#,
        ),
        TEAM,
        public,
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
    pollster::block_on(report(&config, &connected, &ports, "r1"));
    assert_eq!(
        described(&script),
        ["load: o/reviewers=1 o/secret-team=1", "load: o/reviewers=1"],
        "{:?}",
        script.seen()
    );
}
