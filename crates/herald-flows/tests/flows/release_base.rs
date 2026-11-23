//! A release's base branches on each surface: each branch its pull
//! requests merge into held back where a check failed on its head, read
//! once a round on the validators kept from the last read, a public route
//! and `herald queue` without `--private` told of a private repository's
//! as `withheld` says, one that could not be read holding the release
//! (ADR-0083).

use std::fmt::Write as _;

use herald_config::{Uses, load};
use herald_core::{BaseRead, Board, Checks, Holder, Printed};
use herald_flows::{Connected, Ports, prepare, queue, report};
use herald_ports::Response;
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
release = { milestone = "v2.*", header = false WITHHELD }
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
body = "{% if view.readiness %}{{ view.readiness.ready }}{% for h in view.readiness.holds %} {{ h.reason }}:{{ h.repository }}:{{ h.branch }}:{{ h.named | join(\",\") }}{% endfor %}{% endif %}{% for b in view.overviews.bases %} {{ b.repository }}={{ b.state }}{% endfor %}"
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

/// A GraphQL page of the repository `path`, seen as `visibility`, with one
/// open pull request of the release merging into `main`.
fn page(path: &str, visibility: &str) -> String {
    json!({ "data": { "repository": { "visibility": visibility, "pullRequests": { "nodes": [{
        "number": 1, "title": "Change", "url": format!("https://github.com/{path}/pull/1"),
        "isDraft": false, "createdAt": "2026-10-30T10:00:00Z",
        "updatedAt": "2026-10-30T11:00:00Z", "milestone": { "title": "v2.0" },
        "baseRefName": "main",
    }] } } } })
    .to_string()
}

fn runs_url(path: &str) -> String {
    format!("https://api.github.com/repos/{path}/commits/main/check-runs?per_page=100")
}

fn status_url(path: &str) -> String {
    format!("https://api.github.com/repos/{path}/commits/main/status?per_page=100")
}

/// The check runs of a head, `failed` failing and `build` passing.
fn runs(failed: &[&str]) -> String {
    let mut runs = vec![json!({ "name": "build", "status": "completed", "conclusion": "success" })];
    runs.extend(
        failed
            .iter()
            .map(|name| json!({ "name": name, "status": "completed", "conclusion": "failure" })),
    );
    json!({ "total_count": runs.len(), "check_runs": runs }).to_string()
}

const NO_STATUS: &str = r#"{"state":"pending","statuses":[]}"#;

/// The answers reading the base branch `main` of `path`, its check runs
/// failing `failed`.
fn based(path: &str, failed: &[&str]) -> [(String, u16, String); 2] {
    [
        (runs_url(path), 200, runs(failed)),
        (status_url(path), 200, NO_STATUS.to_owned()),
    ]
}

fn connected(
    (paths, withheld): (&[&str], &str),
    script: &Script,
) -> (herald_config::Config, Connected<Script>) {
    let text = TEXT
        .replace("SOURCES", &sources(paths))
        .replace(" WITHHELD", withheld);
    let config = load(
        "herald.toml",
        &text,
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

fn queued(paths: &[&str], (script, store): (&Script, &Failing), printed: Printed) -> Board {
    let (config, connected) = connected((paths, ""), script);
    let clock = FakeClock::at(NOON);
    let ports = Ports {
        store,
        clock: &clock,
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    pollster::block_on(queue(&config, &connected, &ports, ("queue", printed))).unwrap()
}

fn owned(answers: &[(String, u16, String)]) -> Vec<(&str, u16, &str)> {
    answers
        .iter()
        .map(|(url, status, body)| (url.as_str(), *status, body.as_str()))
        .collect()
}

/// What the private route and then the public route are told of the
/// sources `paths`, `withheld` written into the release, each read by its
/// `answers`.
fn told((paths, withheld): (&[&str], &str), answers: &[(String, u16, String)]) -> Vec<String> {
    let script = Script::answering(&owned(answers)).then(&[TEAM, POSTED]);
    let (config, connected) = connected((paths, withheld), &script);
    let store = healthy();
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
                .replace(['\\', '\u{200b}'], "")
        })
        .collect()
}

fn two_repositories() -> Vec<(String, u16, String)> {
    let mut answers = vec![(GRAPHQL.to_owned(), 200, page("o/closed", "PRIVATE"))];
    answers.extend(based("o/closed", &["test"]));
    answers.push((GRAPHQL.to_owned(), 200, page("o/open", "PUBLIC")));
    answers.extend(based("o/open", &[]));
    answers
}

#[test]
fn a_public_route_is_not_told_a_private_repository_s_failing_base_branch() {
    let told_default = told((&["o/closed", "o/open"], ""), &two_repositories());
    assert_eq!(
        told_default,
        [
            "False base:o/closed:main:test o/closed=failed o/open=passed",
            "None o/open=passed",
        ]
    );
    let masked = told(
        (&["o/closed", "o/open"], ", withheld = \"masked\""),
        &two_repositories(),
    );
    assert_eq!(masked[1], "False None:None:None: o/open=passed");
    let shown = told(
        (&["o/closed", "o/open"], ", withheld = \"shown\""),
        &two_repositories(),
    );
    assert_eq!(shown[1], "True o/open=passed");
}

#[test]
fn herald_queue_prints_a_private_base_branch_only_with_private() {
    let script = || Script::answering(&owned(&two_repositories()));
    let store = healthy();
    let withheld = queued(
        &["o/closed", "o/open"],
        (&script(), &store),
        Printed::Withheld,
    );
    assert_eq!(withheld.overviews.bases.len(), 1);
    assert_eq!(withheld.overviews.bases[0].repository, "o/open");
    assert_eq!(withheld.readiness.unwrap().ready(), None);
    let store = healthy();
    let asked = queued(&["o/closed", "o/open"], (&script(), &store), Printed::Asked);
    let readiness = asked.readiness.unwrap();
    assert_eq!(readiness.ready(), Some(false));
    let herald_core::Held::Shown(hold) = &readiness.holds[0] else {
        panic!("a private hold: {readiness:?}");
    };
    assert_eq!(
        (
            hold.repository.as_str(),
            &hold.holder,
            hold.named.as_slice()
        ),
        (
            "o/closed",
            &Holder::Base {
                branch: "main".to_owned()
            },
            ["test".to_owned()].as_slice()
        )
    );
}

#[test]
fn an_unchanged_base_branch_is_read_on_its_kept_validators_and_not_again() {
    let tagged = |url: String, status: u16, body: &str, etag: &str| {
        let response = Response {
            status,
            headers: vec![("etag".to_owned(), etag.to_owned())],
            body: body.as_bytes().to_vec(),
        };
        (url, response)
    };
    let store = healthy();
    let first = Script::answered(vec![
        tagged(GRAPHQL.to_owned(), 200, &page("o/open", "PUBLIC"), "\"g\""),
        tagged(runs_url("o/open"), 200, &runs(&["lint"]), "\"runs\""),
        tagged(status_url("o/open"), 200, NO_STATUS, "\"status\""),
    ]);
    let board = queued(&["o/open"], (&first, &store), Printed::Withheld);
    assert_eq!(
        board.overviews.bases[0].read,
        BaseRead::Checked(Checks::Failed(vec!["lint".to_owned()]))
    );
    let second = Script::answered(vec![
        tagged(GRAPHQL.to_owned(), 200, &page("o/open", "PUBLIC"), "\"g\""),
        tagged(runs_url("o/open"), 304, "", "\"runs\""),
        tagged(status_url("o/open"), 304, "", "\"status\""),
    ]);
    let board = queued(&["o/open"], (&second, &store), Printed::Withheld);
    let asked = second.asked();
    let etag = |headers: &Vec<(String, String)>| {
        headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("if-none-match"))
            .map(|(_, value)| value.clone())
    };
    assert_eq!(etag(&asked[1]).as_deref(), Some("\"runs\""));
    assert_eq!(etag(&asked[2]).as_deref(), Some("\"status\""));
    assert_eq!(
        board.overviews.bases[0].read,
        BaseRead::Checked(Checks::Failed(vec!["lint".to_owned()]))
    );
    assert_eq!(board.readiness.unwrap().ready(), Some(false));
    let key = herald_core::BaseKey {
        source: herald_core::SourceName::new("o-open"),
        repository: "o/open".to_owned(),
        branch: "main".to_owned(),
    };
    let kept = pollster::block_on(herald_ports::Pulls::base(&store, &key))
        .unwrap()
        .unwrap();
    assert_eq!(
        kept.version,
        herald_ports::Version(1),
        "an unchanged branch was written again"
    );
}

#[test]
fn a_base_branch_that_could_not_be_read_holds_the_release_naming_why() {
    let script = Script::answering(&[
        (GRAPHQL, 200, &page("o/open", "PUBLIC")),
        (&runs_url("o/open"), 403, "{}"),
    ]);
    let board = queued(&["o/open"], (&script, &healthy()), Printed::Withheld);
    let readiness = board.readiness.unwrap();
    assert_eq!(readiness.ready(), Some(false));
    let herald_core::Held::Shown(hold) = &readiness.holds[0] else {
        panic!("a private hold: {readiness:?}");
    };
    assert_eq!(hold.held_by, herald_core::HeldBy::Unread);
    assert!(hold.named[0].contains("Checks: read"), "{hold:?}");
    let limited = Script::answering(&[
        (GRAPHQL, 200, &page("o/open", "PUBLIC")),
        (&runs_url("o/open"), 429, "{}"),
    ]);
    let board = queued(&["o/open"], (&limited, &healthy()), Printed::Withheld);
    let BaseRead::Unread(reason) = &board.overviews.bases[0].read else {
        panic!("a base branch read: {board:?}");
    };
    assert!(
        reason.starts_with("its checks could not be read: "),
        "{reason}"
    );
}

#[test]
fn a_view_reads_the_base_branches_of_at_most_twenty_pull_requests_branches() {
    let paths: Vec<String> = (0..21).map(|index| format!("o/r{index:02}")).collect();
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
    let mut answers = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        answers.push((GRAPHQL.to_owned(), 200, page(path, "PUBLIC")));
        if index < 20 {
            answers.extend(based(path, &[]));
        }
    }
    let script = Script::answering(&owned(&answers));
    let board = queued(&paths, (&script, &healthy()), Printed::Withheld);
    let unread: Vec<&str> = board
        .overviews
        .bases
        .iter()
        .filter(|based| matches!(based.read, BaseRead::Unread(_)))
        .map(|based| based.repository.as_str())
        .collect();
    assert_eq!(unread, ["o/r20"]);
    let BaseRead::Unread(reason) = &board.overviews.bases[20].read else {
        panic!("the 21st branch read: {board:?}");
    };
    assert_eq!(
        reason,
        "a release reads the checks of at most 20 base branches"
    );
    assert_eq!(board.overviews.bases.len(), 21);
}

#[test]
fn a_pull_request_the_release_does_not_pick_reads_no_base_branch() {
    let unpicked = page("o/open", "PUBLIC").replace("v2.0", "v9.0");
    let script = Script::answering(&[(GRAPHQL, 200, &unpicked)]);
    let board = queued(&["o/open"], (&script, &healthy()), Printed::Withheld);
    assert_eq!(board.overviews.bases, []);
    assert_eq!(script.seen().len(), 1);
}

#[test]
fn each_branch_of_a_repository_is_read_once_however_many_pull_requests_merge_into_it() {
    let pull = |number: u64, base: &str| {
        json!({
            "number": number, "title": "Change",
            "url": format!("https://github.com/o/open/pull/{number}"),
            "isDraft": false, "createdAt": "2026-10-30T10:00:00Z",
            "updatedAt": "2026-10-30T11:00:00Z", "milestone": { "title": "v2.0" },
            "baseRefName": base,
        })
    };
    let listed = json!({ "data": { "repository": { "visibility": "PUBLIC", "pullRequests": {
        "nodes": [pull(1, "main"), pull(2, "main"), pull(3, "release")],
    } } } })
    .to_string();
    let release_runs = runs_url("o/open").replace("/main/", "/release/");
    let release_status = status_url("o/open").replace("/main/", "/release/");
    let script = Script::answering(&[
        (GRAPHQL, 200, &listed),
        (&runs_url("o/open"), 200, &runs(&[])),
        (&status_url("o/open"), 200, NO_STATUS),
        (&release_runs, 200, &runs(&["test"])),
        (&release_status, 200, NO_STATUS),
    ]);
    let board = queued(&["o/open"], (&script, &healthy()), Printed::Withheld);
    let read: Vec<(&str, &BaseRead)> = board
        .overviews
        .bases
        .iter()
        .map(|based| (based.branch.as_str(), &based.read))
        .collect();
    assert_eq!(
        read,
        [
            ("main", &BaseRead::Checked(Checks::Passed)),
            (
                "release",
                &BaseRead::Checked(Checks::Failed(vec!["test".to_owned()]))
            ),
        ]
    );
    assert_eq!(script.seen().len(), 5);
}

#[test]
fn a_store_keeping_no_pull_requests_reads_each_base_branch_in_full_every_round() {
    let unkept = Failing {
        fault: crate::harness::Fault::KeepsNoPulls,
        ..healthy()
    };
    for _ in 0..2 {
        let mut answers = vec![(GRAPHQL.to_owned(), 200, page("o/open", "PUBLIC"))];
        answers.extend(based("o/open", &["lint"]));
        let script = Script::answering(&owned(&answers));
        let board = queued(&["o/open"], (&script, &unkept), Printed::Withheld);
        assert_eq!(
            board.overviews.bases[0].read,
            BaseRead::Checked(Checks::Failed(vec!["lint".to_owned()]))
        );
        assert!(
            script
                .asked()
                .iter()
                .flatten()
                .all(|(name, _)| !name.eq_ignore_ascii_case("if-none-match"))
        );
    }
}
