use herald_core::Visibility;
use herald_ports::Response;
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::*;
use crate::{SourceOptions, Token};
use herald_core::{Checks, Review};
use herald_ports::Secret;

fn answer(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

fn origin(forge: Forge) -> Origin {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        ..SourceOptions::default()
    };
    Origin::new(forge, &options, "o/r")
}

fn listed(
    forge: Forge,
    tokened: bool,
    (most_open, most_pulls): (usize, usize),
    answers: Vec<Response>,
) -> (Result<Vec<Pull>, Unpolled>, Vec<String>) {
    let origin = origin(forge);
    let token = Token::Named(Secret::new("TOKEN", "t"));
    let opened = Opened {
        origin: &origin,
        token: tokened.then_some(&token),
        project: "r",
        graphql: "https://api.github.com/graphql",
        visibility: Some(Visibility::Public),
        most_open,
        most_pulls,
        stated: false,
    };
    let http = ScriptedHttp::new(answers);
    let erased = Erased(&http as &dyn DynHttp);
    let pulls = pollster::block_on(opened.pulls(&erased));
    let asked = http.asked().into_iter().map(|(url, _)| url).collect();
    (pulls, asked)
}

fn numbers(pulls: Result<Vec<Pull>, Unpolled>) -> Vec<u64> {
    pulls.unwrap().iter().map(|pull| pull.number).collect()
}

const LISTED: (usize, usize) = (MOST_OPEN, 0);

fn github_page(numbers: std::ops::Range<u64>, next: Option<&str>) -> Value {
    let nodes: Vec<Value> = numbers
        .map(|number| {
            json!({
                "number": number, "title": "t", "url": "https://github.com/o/r/pull/1",
                "isDraft": false, "createdAt": "2026-11-18T10:00:00Z",
                "updatedAt": "2026-11-18T11:00:00Z"
            })
        })
        .collect();
    json!({ "data": { "repository": { "visibility": "PUBLIC", "pullRequests": {
        "pageInfo": { "hasNextPage": next.is_some(), "endCursor": next },
        "nodes": nodes
    } } } })
}

fn rest_page(numbers: std::ops::Range<u64>, forge: Forge) -> Value {
    let rows: Vec<Value> = numbers
        .map(|number| match forge {
            Forge::GitLab => json!({
                "iid": number, "title": "t", "web_url": "https://gitlab.com/o/r/-/merge_requests/1",
                "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z"
            }),
            Forge::GitHub | Forge::Forgejo => json!({
                "number": number, "title": "t", "html_url": "https://codeberg.org/o/r/pulls/1",
                "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z"
            }),
        })
        .collect();
    Value::from(rows)
}

#[test]
fn github_pages_through_its_cursor_until_no_page_follows() {
    let (pulls, asked) = listed(
        Forge::GitHub,
        true,
        (MOST_OPEN, MOST_PULLS),
        vec![
            answer(200, &github_page(1..3, Some("c1"))),
            answer(200, &github_page(3..4, None)),
        ],
    );
    assert_eq!(numbers(pulls), [1, 2, 3]);
    assert_eq!(asked, ["https://api.github.com/graphql"; 2]);
}

#[test]
fn a_listing_stops_at_its_most_and_asks_no_page_past_it() {
    let (pulls, asked) = listed(
        Forge::GitHub,
        true,
        (2, MOST_PULLS),
        vec![answer(200, &github_page(1..4, Some("c1")))],
    );
    assert_eq!(numbers(pulls), [1, 2]);
    assert_eq!(asked.len(), 1);
    let (pulls, asked) = listed(
        Forge::GitHub,
        true,
        (PAGE + 1, MOST_PULLS),
        vec![
            answer(200, &github_page(1..3, Some("c1"))),
            answer(200, &github_page(3..5, Some("c2"))),
        ],
    );
    assert_eq!(numbers(pulls), [1, 2, 3, 4]);
    assert_eq!(asked.len(), 2);
}

#[test]
fn gitlab_pages_while_each_page_is_full() {
    let full = PAGE as u64;
    let (pulls, asked) = listed(
        Forge::GitLab,
        false,
        (MOST_OPEN, MOST_PULLS),
        vec![
            answer(200, &rest_page(1..full + 1, Forge::GitLab)),
            answer(200, &rest_page(full + 1..full + 3, Forge::GitLab)),
        ],
    );
    assert_eq!(pulls.unwrap().len(), PAGE + 2);
    assert_eq!(
        asked,
        [
            "https://gitlab.com/api/v4/projects/o%2Fr/merge_requests?state=opened&order_by=updated_at&sort=desc&per_page=100&page=1",
            "https://gitlab.com/api/v4/projects/o%2Fr/merge_requests?state=opened&order_by=updated_at&sort=desc&per_page=100&page=2",
        ]
    );
}

#[test]
fn forgejo_pages_until_a_page_is_empty() {
    let (pulls, asked) = listed(
        Forge::Forgejo,
        true,
        LISTED,
        vec![
            answer(200, &rest_page(1..3, Forge::Forgejo)),
            answer(200, &rest_page(3..3, Forge::Forgejo)),
        ],
    );
    assert_eq!(numbers(pulls), [1, 2]);
    assert_eq!(asked.len(), 2);
    assert!(asked[0].ends_with("/repos/o/r/pulls?state=open&sort=recentupdate&limit=100&page=1"));
    assert!(asked[1].ends_with("&page=2"));
}

#[test]
fn a_page_that_fails_or_does_not_read_fails_the_listing() {
    let (refused, _) = listed(Forge::GitLab, false, LISTED, vec![answer(403, &json!({}))]);
    assert!(matches!(refused, Err(Unpolled::Unreadable(_))));
    let (unread, _) = listed(Forge::GitLab, false, LISTED, vec![answer(200, &json!({}))]);
    assert!(matches!(unread, Err(Unpolled::Unreadable(_))));
    let (unreached, _) = listed(Forge::Forgejo, false, LISTED, Vec::new());
    assert!(matches!(unreached, Err(Unpolled::Unreachable(_))));
}

fn headed(numbers: std::ops::Range<u64>) -> Value {
    let rows: Vec<Value> = numbers
        .map(|number| {
            json!({
                "number": number, "title": "t", "html_url": "https://github.com/o/r/pull/1",
                "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
                "requested_reviewers": [ { "login": "bob" } ],
                "head": { "sha": format!("s{number}") }
            })
        })
        .collect();
    Value::from(rows)
}

#[test]
fn github_without_a_token_lists_by_rest_and_reads_each_in_full_up_to_its_most() {
    let (pulls, asked) = listed(
        Forge::GitHub,
        false,
        (MOST_OPEN, 1),
        vec![
            answer(200, &headed(1..3)),
            answer(
                200,
                &json!([ { "user": { "login": "bob" }, "state": "APPROVED", "author_association": "MEMBER" } ]),
            ),
            answer(
                200,
                &json!({ "check_runs": [ { "name": "test", "status": "completed", "conclusion": "success" } ] }),
            ),
        ],
    );
    let pulls = pulls.unwrap();
    assert_eq!(
        asked,
        [
            "https://api.github.com/repos/o/r/pulls?state=open&sort=updated&direction=desc&per_page=100&page=1",
            "https://api.github.com/repos/o/r/pulls/1/reviews?per_page=100",
            "https://api.github.com/repos/o/r/commits/s1/check-runs?per_page=100",
        ]
    );
    let first = pulls.first().unwrap();
    assert_eq!(
        (first.review, first.checks.clone(), first.unread),
        (Review::Approved, Checks::Passed, false)
    );
    let second = pulls.get(1).unwrap();
    assert_eq!(
        (second.review, second.checks.clone(), second.unread),
        (Review::Requested, Checks::Absent, true)
    );
}

#[test]
fn forgejo_reads_each_in_full_and_one_with_no_head_reads_no_checks() {
    let (pulls, asked) = listed(
        Forge::Forgejo,
        false,
        (MOST_OPEN, MOST_PULLS),
        vec![
            answer(200, &rest_page(1..2, Forge::Forgejo)),
            answer(200, &json!([])),
            answer(200, &json!([])),
        ],
    );
    assert_eq!(numbers(pulls), [1]);
    assert_eq!(
        asked,
        [
            "https://codeberg.org/api/v1/repos/o/r/pulls?state=open&sort=recentupdate&limit=100&page=1",
            "https://codeberg.org/api/v1/repos/o/r/pulls?state=open&sort=recentupdate&limit=100&page=2",
            "https://codeberg.org/api/v1/repos/o/r/pulls/1/reviews?limit=100",
        ]
    );
}

#[test]
fn a_detail_that_fails_or_does_not_read_fails_the_listing() {
    for answers in [
        vec![answer(200, &headed(1..2)), answer(404, &json!({}))],
        vec![answer(200, &headed(1..2)), answer(200, &json!({}))],
        vec![
            answer(200, &headed(1..2)),
            answer(200, &json!([])),
            answer(200, &json!("runs")),
        ],
    ] {
        let (failed, _) = listed(Forge::GitHub, false, (MOST_OPEN, MOST_PULLS), answers);
        assert!(matches!(failed, Err(Unpolled::Unreadable(_))));
    }
}

#[test]
fn each_forge_and_credential_reads_by_its_path_at_its_cost() {
    assert_eq!(ReadPath::of(Forge::GitHub, true), ReadPath::Graphql);
    assert_eq!(ReadPath::of(Forge::GitHub, false), ReadPath::Detailed);
    assert_eq!(ReadPath::of(Forge::GitLab, true), ReadPath::Listing);
    assert_eq!(ReadPath::of(Forge::GitLab, false), ReadPath::Listing);
    assert_eq!(ReadPath::of(Forge::Forgejo, true), ReadPath::Detailed);
    assert_eq!(ReadPath::of(Forge::Forgejo, false), ReadPath::Detailed);
    let names: Vec<&str> = [ReadPath::Graphql, ReadPath::Listing, ReadPath::Detailed]
        .into_iter()
        .map(ReadPath::name)
        .collect();
    assert_eq!(names, ["graphql", "listing", "rest"]);
    assert_eq!(ReadPath::Graphql.most_requests(MOST_OPEN, MOST_PULLS), 5);
    assert_eq!(ReadPath::Listing.most_requests(101, MOST_PULLS), 2);
    assert_eq!(ReadPath::Detailed.most_requests(MOST_OPEN, MOST_PULLS), 205);
    assert_eq!(ReadPath::Detailed.most_requests(50, MOST_PULLS), 101);
    assert_eq!(ReadPath::Detailed.most_requests(100, 100), 201);
}

#[test]
fn a_cost_names_its_path_requests_and_whether_it_reads_mergeability() {
    let most = herald_kinds::Most::default();
    let costs: Vec<(&str, usize, bool, bool)> = [
        (Forge::GitHub, true),
        (Forge::GitHub, false),
        (Forge::GitLab, false),
        (Forge::Forgejo, true),
    ]
    .into_iter()
    .map(|(forge, tokened)| {
        let cost = cost(forge, tokened, most);
        (cost.path, cost.requests, cost.detailed, cost.mergeability)
    })
    .collect();
    assert_eq!(
        costs,
        [
            ("graphql", 5, false, true),
            ("rest", 205, true, false),
            ("listing", 5, false, true),
            ("rest", 205, true, true)
        ]
    );
}

#[test]
fn a_queued_repository_reads_its_pulls_through_its_own_path_at_its_most() {
    let queued = Queued {
        origin: origin(Forge::GitHub),
        token: Some(Token::Named(Secret::new("TOKEN", "t"))),
        project: "r".to_owned(),
        graphql: "https://ghe.example/api/graphql".to_owned(),
    };
    let http = ScriptedHttp::new(vec![answer(200, &github_page(1..3, None))]);
    let pulls = pollster::block_on(herald_kinds::Queue::opened(
        &queued,
        &http,
        Asked {
            most: herald_kinds::Most { open: 1, pulls: 0 },
            ..Asked::default()
        },
    ));
    assert_eq!(numbers(pulls), [1]);
    let asked: Vec<String> = http.asked().into_iter().map(|(url, _)| url).collect();
    assert_eq!(asked, ["https://ghe.example/api/graphql"]);
    assert!(!herald_kinds::Queue::scoped(&queued));
}

/// The visibility each row of a live listing of `forge` read with or
/// without a token, as `stated` asks, where the listing states `named`.
fn stated_rows(forge: Forge, tokened: bool, stated: bool, named: &str) -> Vec<Visibility> {
    let queued = Queued {
        origin: origin(forge),
        token: tokened.then(|| Token::Named(Secret::new("TOKEN", "t"))),
        project: "r".to_owned(),
        graphql: "https://api.github.com/graphql".to_owned(),
    };
    let row = json!({
        "number": 1, "title": "t", "html_url": "https://codeberg.org/o/r/pulls/1",
        "iid": 1, "web_url": "https://gitlab.com/o/r/-/merge_requests/1",
        "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
        "base": { "repo": { "id": 42, "visibility": named } }
    });
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([row])),
        answer(200, &json!([])),
        answer(200, &json!([])),
    ]);
    let asked = Asked {
        most: herald_kinds::Most { open: 1, pulls: 0 },
        scope: None,
        stated,
    };
    pollster::block_on(herald_kinds::Queue::opened(&queued, &http, asked))
        .unwrap()
        .iter()
        .map(|pull| pull.visibility)
        .collect()
}

#[test]
fn a_tokenless_rest_listing_s_stated_visibility_is_taken_only_where_the_view_says() {
    for forge in [Forge::GitHub, Forge::Forgejo] {
        assert_eq!(
            stated_rows(forge, false, false, "public"),
            [Visibility::Private]
        );
        assert_eq!(
            stated_rows(forge, false, true, "public"),
            [Visibility::Public]
        );
        assert_eq!(
            stated_rows(forge, false, true, "internal"),
            [Visibility::Internal]
        );
    }
    assert_eq!(
        stated_rows(Forge::Forgejo, true, true, "public"),
        [Visibility::Private]
    );
    assert_eq!(
        stated_rows(Forge::GitLab, false, true, "public"),
        [Visibility::Private]
    );
}

#[test]
fn a_kept_listing_never_takes_the_stated_visibility_for_its_rows() {
    let queued = Queued {
        origin: origin(Forge::GitHub),
        token: None,
        project: "r".to_owned(),
        graphql: "https://api.github.com/graphql".to_owned(),
    };
    let row = json!({
        "number": 1, "title": "t", "html_url": "https://github.com/o/r/pull/1",
        "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
        "base": { "repo": { "id": 42, "visibility": "public" } }
    });
    let http = ScriptedHttp::new(vec![answer(200, &json!([row]))]);
    let asked = Asked {
        stated: true,
        ..Asked::default()
    };
    let relisted =
        pollster::block_on(herald_kinds::Queue::listed(&queued, &http, asked, &[])).unwrap();
    let herald_kinds::Relisted::Listed {
        pulls, visibility, ..
    } = relisted
    else {
        panic!("{relisted:?}");
    };
    assert_eq!(visibility, Some(Visibility::Public));
    assert_eq!(pulls.first().unwrap().visibility, Visibility::Private);
}

#[test]
fn a_failed_listing_names_what_it_read_on_each_forge_and_path() {
    let named = |forge, tokened| {
        let (failed, _) = listed(forge, tokened, LISTED, vec![answer(500, &json!({}))]);
        match failed {
            Err(Unpolled::Unreadable(why)) => why,
            other => panic!("{other:?}"),
        }
    };
    assert!(named(Forge::GitHub, true).starts_with(github::WHAT));
    assert!(named(Forge::GitHub, false).starts_with(listing::GITHUB));
    assert!(named(Forge::GitLab, false).starts_with(gitlab::WHAT));
    assert!(named(Forge::Forgejo, true).starts_with(listing::FORGEJO));
}

#[test]
fn github_without_a_token_pages_while_each_page_is_full() {
    let (pulls, asked) = listed(
        Forge::GitHub,
        false,
        LISTED,
        vec![answer(200, &headed(0..100)), answer(200, &headed(100..101))],
    );
    assert_eq!(pulls.unwrap().len(), 101);
    assert_eq!(asked.len(), 2);
}

#[test]
fn a_pull_request_with_a_reviewer_asked_and_no_review_given_waits_on_review() {
    let (pulls, _) = listed(
        Forge::GitHub,
        false,
        (MOST_OPEN, 1),
        vec![
            answer(200, &headed(1..2)),
            answer(200, &json!([])),
            answer(200, &json!({ "check_runs": [] })),
        ],
    );
    assert_eq!(pulls.unwrap()[0].review, Review::Requested);
}

#[test]
fn a_kept_listing_never_takes_the_visibility_a_tokenless_listing_states() {
    let queued = Queued {
        origin: origin(Forge::GitHub),
        token: None,
        project: "r".to_owned(),
        graphql: "https://api.github.com/graphql".to_owned(),
    };
    assert!(!herald_kinds::Queue::scoped(&queued));
    let row = json!({
        "number": 1, "title": "t", "html_url": "https://github.com/o/r/pull/1",
        "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
        "base": { "repo": { "id": 42, "visibility": "public" } }
    });
    let http = ScriptedHttp::new(vec![answer(200, &json!([row]))]);
    let asked = Asked {
        most: herald_kinds::Most { open: 1, pulls: 0 },
        scope: None,
        stated: true,
    };
    let listed =
        pollster::block_on(herald_kinds::Queue::listed(&queued, &http, asked, &[])).unwrap();
    let herald_kinds::Relisted::Listed { pulls, .. } = listed else {
        panic!("{listed:?}");
    };
    assert_eq!(
        pulls.iter().map(|pull| pull.visibility).collect::<Vec<_>>(),
        [Visibility::Private]
    );
}
