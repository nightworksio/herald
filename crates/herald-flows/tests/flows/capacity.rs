//! A view's capacity overview: the runs queued and running of the
//! repositories it shows, each read charged to the run's budget, a
//! repository the token may not read named unread, and each audience
//! shown only the runs it may see (ADR-0066).

use herald_config::{Config, Uses, load};
use herald_core::{Board, Visibility};
use herald_flows::{Budget, Connected, Ports, Requests, Stop, Sweep, prepare, report};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use serde_json::json;

use crate::harness::{NOON, POSTED, Script, healthy};

const GRAPHQL: &str = "https://api.github.com/graphql";
const QUEUED: &str = "/repos/o/open/actions/runs?status=queued&per_page=100";
const RUNNING: &str = "/repos/o/open/actions/runs?status=in_progress&per_page=100";

fn config(view: &str) -> Config {
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
[views.ci]
sources = ["open"]
overviews = ["capacity"]
VIEW
[reports.ci]
view = "ci"
every = "1h"
at = "00:00"
[templates.t]
title = "{{ view.name }}"
body = "{% if view.overviews.capacity %}queued:{{ view.overviews.capacity.queued }} running:{{ view.overviews.capacity.running }} superseded:{{ view.overviews.capacity.superseded | length }}{% endif %} unread:{{ view.overviews.unread | length }}"
[[routes]]
id = "private"
sources = ["ci"]
destination = "discord"
channel = "1566200000000000100"
template = "t"
private = true
"#
    .replace("VIEW", view);
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

/// A GraphQL page of `o/open`, seen as `visibility`, with one open pull
/// request on the head `h1`.
fn page(visibility: &str) -> String {
    json!({ "data": { "repository": { "visibility": visibility, "pullRequests": { "nodes": [ {
        "number": 1, "title": "Change", "url": "https://github.com/o/open/pull/1",
        "isDraft": false, "createdAt": "2026-10-20T10:00:00Z", "updatedAt": "2026-10-30T11:00:00Z",
        "headRefOid": "h1",
        "author": { "__typename": "User", "login": "ana" },
        "reviewDecision": "REVIEW_REQUIRED",
        "reviewRequests": { "totalCount": 0, "nodes": [] }
    } ] } } } })
    .to_string()
}

fn runs(listed: &[(u64, &str)]) -> String {
    let runs: Vec<_> = listed
        .iter()
        .map(|(id, sha)| {
            json!({ "id": id, "html_url": format!("https://github.com/o/open/actions/runs/{id}"),
                "head_sha": sha, "event": "pull_request", "created_at": "2026-11-20T10:00:00Z" })
        })
        .collect();
    json!({ "workflow_runs": runs }).to_string()
}

/// The board of the view `ci`, or the report's sweep where `reported`
/// says, its outbound requests bounded to `most` where it says.
fn ran(script: &Script, most: Option<usize>) -> (Result<Board, Stop>, Vec<herald_flows::Swept>) {
    ran_within(script, (most, ""))
}

/// As `ran`, the view `ci` also set by `view`.
fn ran_within(
    script: &Script,
    (most, view): (Option<usize>, &str),
) -> (Result<Board, Stop>, Vec<herald_flows::Swept>) {
    let config = config(view);
    let store = healthy();
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
        budget: Budget {
            requests: most.map(|most| Requests {
                made: script.made(),
                from: script.made().load(std::sync::atomic::Ordering::Relaxed),
                most,
                spare: 0,
            }),
            ..Budget::UNBOUNDED
        },
    };
    let board = pollster::block_on(herald_flows::queue(
        &config,
        &connected,
        &ports,
        ("ci", false),
    ));
    let swept = if board.is_ok() && most.is_none() {
        pollster::block_on(report(&config, &connected, &ports, "r1"))
    } else {
        Vec::new()
    };
    (board, swept)
}

#[test]
fn a_view_counts_its_repositories_runs_and_names_those_superseded() {
    let page = page("PUBLIC");
    let (queued, running) = (runs(&[(1, "h1")]), runs(&[(2, "h0"), (3, "h1")]));
    let script = Script::answering(&[
        (GRAPHQL, 200, &page),
        (QUEUED, 200, &queued),
        (RUNNING, 200, &running),
        (GRAPHQL, 200, &page),
        (QUEUED, 200, &queued),
        (RUNNING, 200, &running),
        POSTED,
    ]);
    let (board, swept) = ran(&script, None);
    let board = board.unwrap();
    let capacity = board.overviews.capacity.as_ref().unwrap();
    assert_eq!(capacity.len(), 3);
    assert!(
        capacity
            .iter()
            .all(|run| run.visibility == Visibility::Public)
    );
    let superseded: Vec<&str> = board
        .overviews
        .superseded()
        .into_iter()
        .map(|run| run.id.as_str())
        .collect();
    assert_eq!(superseded, ["2"]);
    assert!(
        matches!(swept[0].outcome, Sweep::Reported { .. }),
        "{swept:?}"
    );
    let posted = String::from_utf8(script.bodies().last().unwrap().clone())
        .unwrap()
        .replace('\\', "");
    assert!(
        posted.contains("queued:1 running:2 superseded:1 unread:0"),
        "{posted}"
    );
}

#[test]
fn a_repository_the_token_may_not_read_is_named_unread_never_counted_as_idle() {
    let page = page("PRIVATE");
    let script = Script::answering(&[(GRAPHQL, 200, &page), (QUEUED, 403, "{}")]);
    let (board, _) = ran(&script, Some(10));
    let board = board.unwrap();
    assert_eq!(board.overviews.capacity, Some(Vec::new()));
    assert_eq!(board.overviews.unread.len(), 1);
    assert_eq!(board.overviews.unread[0].visibility, Visibility::Private);
    assert!(board.overviews.unread[0].reason.contains("Actions: read"));
}

#[test]
fn each_request_a_capacity_read_makes_is_charged_and_none_is_made_past_the_budget() {
    let page = page("PUBLIC");
    let queued = runs(&[]);
    let script = Script::answering(&[
        (GRAPHQL, 200, &page),
        (QUEUED, 200, &queued),
        (RUNNING, 200, &queued),
    ]);
    let (board, _) = ran(&script, Some(2));
    let unread = board.unwrap().overviews.unread;
    assert_eq!(unread.len(), 1);
    assert!(
        unread[0]
            .reason
            .starts_with("the run's request budget ended before its runs were read"),
        "{unread:?}"
    );
    assert_eq!(script.seen().len(), 2);
}

#[test]
fn a_view_scoped_to_other_repositories_reads_and_names_nothing_of_this_one() {
    let page = page("PRIVATE");
    let script = Script::answering(&[(GRAPHQL, 200, &page)]);
    let (board, _) = ran_within(&script, (Some(10), "repositories = [\"o/elsewhere*\"]"));
    let board = board.unwrap();
    assert_eq!(board.overviews.capacity, Some(Vec::new()));
    assert_eq!(board.overviews.unread, []);
    assert_eq!(script.seen().len(), 1);
}

#[test]
fn a_capped_listing_says_it_holds_more_than_it_counts() {
    let page = page("PUBLIC");
    let full: Vec<(u64, &str)> = (1..=100).map(|id| (id, "h1")).collect();
    let (full, none) = (runs(&full), runs(&[]));
    let script = Script::answering(&[
        (GRAPHQL, 200, &page),
        (QUEUED, 200, &full),
        (RUNNING, 200, &none),
    ]);
    let (board, _) = ran(&script, Some(10));
    let overviews = board.unwrap().overviews;
    assert_eq!(overviews.capacity.map(|runs| runs.len()), Some(100));
    assert_eq!(overviews.unread.len(), 1);
    assert!(
        overviews.unread[0]
            .reason
            .starts_with("a listing held more than 100 of its runs"),
        "{:?}",
        overviews.unread
    );
}

#[test]
fn a_source_whose_runs_fail_to_read_is_named_unread_with_what_failed() {
    let page = page("PUBLIC");
    let failing = Script::answering(&[(GRAPHQL, 200, &page), (QUEUED, 500, "{}")]);
    let (board, _) = ran(&failing, Some(10));
    let unread = &board.unwrap().overviews.unread;
    assert_eq!(unread[0].repository, "open");
    assert_eq!(unread[0].visibility, Visibility::Private);
    assert!(
        unread[0].reason.starts_with("its runs could not be read: "),
        "{unread:?}"
    );
    let unscripted = Script::answering(&[(GRAPHQL, 200, &page)]);
    let (board, _) = ran(&unscripted, Some(10));
    let unread = &board.unwrap().overviews.unread;
    assert!(
        unread[0]
            .reason
            .starts_with("its forge could not be reached: "),
        "{unread:?}"
    );
}

#[test]
fn a_scoped_view_whose_capacity_read_fails_shows_the_section_unread_never_clean() {
    let page = page("PUBLIC");
    let script = Script::answering(&[(GRAPHQL, 200, &page), (QUEUED, 500, "{}")]);
    let (board, _) = ran_within(&script, (Some(10), "repositories = [\"o/open\"]"));
    let overviews = board.unwrap().overviews;
    assert_eq!(overviews.capacity, Some(Vec::new()));
    assert_eq!(overviews.unread.len(), 1);
    assert_eq!(overviews.unread[0].repository, "open");
    assert!(
        overviews.unread[0]
            .reason
            .starts_with("its runs could not be read: ")
    );
}
