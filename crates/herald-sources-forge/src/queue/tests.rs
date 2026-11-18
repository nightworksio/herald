use herald_core::Visibility;
use herald_ports::Response;
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::*;
use crate::SourceOptions;

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
    most: usize,
    answers: Vec<Response>,
) -> (Result<Vec<u64>, Unpolled>, Vec<String>) {
    let origin = origin(forge);
    let opened = Opened {
        origin: &origin,
        token: None,
        project: "r",
        graphql: "https://api.github.com/graphql",
        visibility: Some(Visibility::Public),
        most,
    };
    let http = ScriptedHttp::new(answers);
    let erased = Erased(&http as &dyn DynHttp);
    let pulls = pollster::block_on(opened.pulls(&erased))
        .map(|pulls| pulls.iter().map(|pull| pull.number).collect());
    let asked = http.asked().into_iter().map(|(url, _)| url).collect();
    (pulls, asked)
}

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
        MOST_PULLS,
        vec![
            answer(200, &github_page(1..3, Some("c1"))),
            answer(200, &github_page(3..4, None)),
        ],
    );
    assert_eq!(pulls.unwrap(), [1, 2, 3]);
    assert_eq!(asked, ["https://api.github.com/graphql"; 2]);
}

#[test]
fn a_listing_stops_at_its_most_and_asks_no_page_past_it() {
    let (pulls, asked) = listed(
        Forge::GitHub,
        2,
        vec![answer(200, &github_page(1..4, Some("c1")))],
    );
    assert_eq!(pulls.unwrap(), [1, 2]);
    assert_eq!(asked.len(), 1);
    let (pulls, asked) = listed(
        Forge::GitHub,
        PAGE + 1,
        vec![
            answer(200, &github_page(1..3, Some("c1"))),
            answer(200, &github_page(3..5, Some("c2"))),
        ],
    );
    assert_eq!(pulls.unwrap(), [1, 2, 3, 4]);
    assert_eq!(asked.len(), 2);
}

#[test]
fn gitlab_pages_while_each_page_is_full() {
    let full = PAGE as u64;
    let (pulls, asked) = listed(
        Forge::GitLab,
        MOST_PULLS,
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
        MOST_PULLS,
        vec![
            answer(200, &rest_page(1..3, Forge::Forgejo)),
            answer(200, &rest_page(3..3, Forge::Forgejo)),
        ],
    );
    assert_eq!(pulls.unwrap(), [1, 2]);
    assert_eq!(asked.len(), 2);
    assert!(asked[0].ends_with("/repos/o/r/pulls?state=open&sort=recentupdate&limit=100&page=1"));
    assert!(asked[1].ends_with("&page=2"));
}

#[test]
fn a_page_that_fails_or_does_not_read_fails_the_listing() {
    let (refused, _) = listed(Forge::GitLab, MOST_PULLS, vec![answer(403, &json!({}))]);
    assert!(matches!(refused, Err(Unpolled::Unreadable(_))));
    let (unread, _) = listed(Forge::GitLab, MOST_PULLS, vec![answer(200, &json!({}))]);
    assert!(matches!(unread, Err(Unpolled::Unreadable(_))));
    let (unreached, _) = listed(Forge::Forgejo, MOST_PULLS, Vec::new());
    assert!(matches!(unreached, Err(Unpolled::Unreachable(_))));
}
