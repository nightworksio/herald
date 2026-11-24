use herald_core::{Glob, Header, HeaderRead, MOST_ISSUE_ROWS, Visibility};
use herald_kinds::Unpolled;
use herald_ports::{DynHttp, Erased, Response};
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

fn milestone(forge: Forge, number: u64, title: &str) -> Value {
    match forge {
        Forge::GitLab => json!({
            "id": number, "iid": number + 100, "title": title, "description": null,
            "state": "active", "project_id": 7, "group_id": null, "due_date": null,
            "created_at": "2026-11-18T08:00:00Z", "updated_at": null,
            "web_url": format!("https://gitlab.com/o/r/-/milestones/{}", number + 100),
        }),
        Forge::GitHub | Forge::Forgejo => json!({
            "id": 1000 + number, "number": number, "title": title, "description": null,
            "state": "open", "open_issues": 2, "closed_issues": 1,
            "created_at": "2026-11-18T08:00:00Z", "updated_at": null, "closed_at": null,
            "due_on": null, "html_url": format!("https://github.com/o/r/milestone/{number}"),
        }),
    }
}

fn issue(forge: Forge, number: u64, extra: &Value) -> Value {
    let mut issue = match forge {
        Forge::GitLab => json!({
            "iid": number, "title": format!("Issue {number}"), "state": "opened",
            "web_url": format!("https://gitlab.com/o/r/-/issues/{number}"),
            "author": { "username": "ann" }, "created_at": "2026-11-18T08:00:00Z",
            "updated_at": null, "closed_at": null, "labels": [],
        }),
        Forge::GitHub | Forge::Forgejo => json!({
            "number": number, "title": format!("Issue {number}"), "state": "open",
            "html_url": format!("https://github.com/o/r/issues/{number}"),
            "user": { "login": "ann", "type": "User" }, "state_reason": null,
            "created_at": "2026-11-18T08:00:00Z", "updated_at": null, "closed_at": null,
        }),
    };
    issue
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    issue
}

fn issues(forge: Forge, numbers: std::ops::Range<u64>) -> Value {
    Value::from(
        numbers
            .map(|number| issue(forge, number, &json!({})))
            .collect::<Vec<_>>(),
    )
}

fn read(
    forge: Forge,
    http: &ScriptedHttp,
    (header, page): (bool, Option<usize>),
) -> Result<Header, Unpolled> {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        page: page.and_then(std::num::NonZeroUsize::new),
        ..SourceOptions::default()
    };
    let origin = Origin::new(forge, &options, "o/r");
    let glob = Glob::new("v2.*").unwrap();
    let http = Erased(http as &dyn DynHttp);
    let heading = HeaderRead {
        header,
        issues: true,
    };
    pollster::block_on(super::super::header(
        &http,
        (&origin, None),
        (&glob, Visibility::Internal),
        (heading, "2026-11-23T12:00:00Z".parse().unwrap()),
    ))
}

fn urls(http: &ScriptedHttp) -> Vec<String> {
    http.asked().into_iter().map(|(url, _)| url).collect()
}

fn numbers(header: &Header) -> Vec<u64> {
    header
        .issues
        .iter()
        .flatten()
        .map(|issue| issue.number)
        .collect()
}

#[test]
fn a_github_release_lists_the_open_issues_of_each_milestone_matched_leaving_out_pull_requests() {
    let listed = json!([
        milestone(Forge::GitHub, 1, "v2.0"),
        milestone(Forge::GitHub, 2, "v1.0"),
        milestone(Forge::GitHub, 3, "v2.1"),
    ]);
    let carried = json!([
        issue(Forge::GitHub, 7, &json!({})),
        issue(Forge::GitHub, 8, &json!({ "pull_request": {} })),
    ]);
    let http = ScriptedHttp::new(vec![
        answer(200, &listed),
        answer(200, &carried),
        answer(200, &json!([issue(Forge::GitHub, 9, &json!({}))])),
    ]);
    let header = read(Forge::GitHub, &http, (true, None)).unwrap();
    assert_eq!(
        urls(&http),
        [
            "https://api.github.com/repos/o/r/milestones?state=open&per_page=30",
            "https://api.github.com/repos/o/r/issues?milestone=1&state=open&per_page=30",
            "https://api.github.com/repos/o/r/issues?milestone=3&state=open&per_page=30",
        ]
    );
    assert_eq!(numbers(&header), [7, 9]);
    let first = &header.issues.as_ref().unwrap()[0];
    assert_eq!(
        (
            first.repository.as_str(),
            first.visibility,
            first.title.as_str()
        ),
        ("o/r", Visibility::Internal, "Issue 7")
    );
    assert_eq!(first.url, "https://github.com/o/r/issues/7");
    assert_eq!(first.milestone, "v2.0");
    assert_eq!(header.issues.as_ref().unwrap()[1].milestone, "v2.1");
    assert_eq!(header.milestones.len(), 2);
    assert!(header.unread.is_empty(), "{header:?}");
}

#[test]
fn a_release_with_no_header_lists_issues_and_names_no_milestone_nor_counts_them() {
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([milestone(Forge::GitLab, 1, "v2.0")])),
        answer(200, &issues(Forge::GitLab, 4..5)),
    ]);
    let header = read(Forge::GitLab, &http, (false, None)).unwrap();
    assert_eq!(
        urls(&http),
        [
            "https://gitlab.com/api/v4/projects/o%2Fr/milestones?state=active&per_page=30",
            "https://gitlab.com/api/v4/projects/o%2Fr/issues?milestone=v2.0&state=opened&per_page=30",
        ]
    );
    assert_eq!(header.milestones, []);
    assert_eq!(numbers(&header), [4]);
}

#[test]
fn a_confidential_gitlab_issue_is_seen_as_private() {
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([milestone(Forge::GitLab, 1, "v2.0 beta")])),
        answer(
            200,
            &json!([
                issue(Forge::GitLab, 4, &json!({ "confidential": true })),
                issue(Forge::GitLab, 5, &json!({ "confidential": false })),
            ]),
        ),
        answer(
            200,
            &json!({ "statistics": { "counts": { "closed": 1, "opened": 2 } } }),
        ),
    ]);
    let header = read(Forge::GitLab, &http, (true, None)).unwrap();
    assert!(urls(&http)[1].ends_with("/issues?milestone=v2.0%20beta&state=opened&per_page=30"));
    let seen: Vec<(u64, Visibility)> = header
        .issues
        .iter()
        .flatten()
        .map(|issue| (issue.number, issue.visibility))
        .collect();
    assert_eq!(seen, [(4, Visibility::Private), (5, Visibility::Internal)]);
}

#[test]
fn a_forgejo_release_lists_issues_by_the_milestone_s_id_and_its_limit() {
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([milestone(Forge::Forgejo, 1, "v2.0")])),
        answer(200, &issues(Forge::Forgejo, 4..6)),
    ]);
    let header = read(Forge::Forgejo, &http, (true, None)).unwrap();
    assert!(
        urls(&http)[1]
            .ends_with("/repos/o/r/issues?state=open&type=issues&milestones=1001&limit=30")
    );
    assert_eq!(numbers(&header), [4, 5]);
}

#[test]
fn a_refused_or_unseen_issue_listing_names_the_repository_unread_with_why() {
    for (forge, status, reason) in [
        (Forge::GitHub, 403, GITHUB_ISSUES_REFUSED),
        (Forge::GitHub, 401, GITHUB_ISSUES_REFUSED),
        (Forge::GitLab, 403, GITLAB_ISSUES_REFUSED),
        (Forge::GitLab, 401, GITLAB_ISSUES_REFUSED),
        (Forge::Forgejo, 403, FORGEJO_ISSUES_REFUSED),
        (Forge::Forgejo, 401, FORGEJO_ISSUES_REFUSED),
        (Forge::GitHub, 404, UNSEEN),
        (Forge::GitLab, 404, UNSEEN),
        (Forge::Forgejo, 404, UNSEEN),
    ] {
        let http = ScriptedHttp::new(vec![
            answer(
                200,
                &json!([milestone(forge, 1, "v2.0"), milestone(forge, 2, "v2.1")]),
            ),
            answer(status, &json!({ "message": "no" })),
            answer(status, &json!({ "message": "no" })),
        ]);
        let header = read(forge, &http, (false, None)).unwrap();
        assert_eq!(http.asked().len(), 3, "{forge:?} {status}");
        assert_eq!(numbers(&header), Vec::<u64>::new(), "{forge:?} {status}");
        assert_eq!(header.issues, Some(Vec::new()));
        assert_eq!(header.unread.len(), 1, "{forge:?} {status}");
        assert_eq!(header.unread[0].reason, reason, "{forge:?} {status}");
        assert_eq!(header.unread[0].visibility, Visibility::Internal);
    }
}

#[test]
fn a_rate_limited_or_unreadable_issue_listing_stops_the_read() {
    for forge in [Forge::GitHub, Forge::GitLab, Forge::Forgejo] {
        let http = ScriptedHttp::new(vec![
            answer(200, &json!([milestone(forge, 1, "v2.0")])),
            answer(429, &json!({})),
        ]);
        assert!(read(forge, &http, (false, None)).is_err(), "{forge:?}");
        let http = ScriptedHttp::new(vec![
            answer(200, &json!([milestone(forge, 1, "v2.0")])),
            answer(200, &json!({ "not": "a listing" })),
        ]);
        assert!(read(forge, &http, (false, None)).is_err(), "{forge:?}");
        let http = ScriptedHttp::new(vec![
            answer(200, &json!([milestone(forge, 1, "v2.0")])),
            answer(500, &json!({ "message": "down" })),
        ]);
        assert!(read(forge, &http, (false, None)).is_err(), "{forge:?}");
    }
}

#[test]
fn a_page_the_listing_fills_names_the_repository_unread_counting_pull_requests_too() {
    let milestones = json!([
        milestone(Forge::GitHub, 1, "v2.0"),
        milestone(Forge::GitHub, 2, "v1.0"),
        milestone(Forge::GitHub, 3, "v1.1")
    ]);
    for (listed, filled) in [
        (json!([issue(Forge::GitHub, 4, &json!({}))]), false),
        (
            json!([
                issue(Forge::GitHub, 4, &json!({})),
                issue(Forge::GitHub, 5, &json!({}))
            ]),
            true,
        ),
        (
            json!([
                issue(Forge::GitHub, 4, &json!({})),
                issue(Forge::GitHub, 5, &json!({ "pull_request": {} }))
            ]),
            true,
        ),
    ] {
        let http = ScriptedHttp::new(vec![answer(200, &milestones), answer(200, &listed)]);
        let header = read(Forge::GitHub, &http, (false, Some(4))).unwrap();
        assert!(header.unread.is_empty(), "{header:?}");
        let http = ScriptedHttp::new(vec![answer(200, &milestones), answer(200, &listed)]);
        let header = read(Forge::GitHub, &http, (false, Some(2))).unwrap();
        let named: Vec<&str> = header
            .unread
            .iter()
            .map(|unread| unread.reason.as_str())
            .collect();
        let reason = "its milestone v2.0 listed more than 2 open issues; only those 2 are read";
        assert_eq!(named.contains(&reason), filled, "{named:?}");
    }
}

#[test]
fn a_repository_names_at_most_the_most_issue_rows_and_says_how_many_carry_its_milestones() {
    let most = u64::try_from(MOST_ISSUE_ROWS).unwrap();
    for (second, over) in [
        (most / 2 - 1, false),
        (most / 2, false),
        (most / 2 + 1, true),
    ] {
        let http = ScriptedHttp::new(vec![
            answer(
                200,
                &json!([
                    milestone(Forge::GitLab, 1, "v2.0"),
                    milestone(Forge::GitLab, 2, "v2.1")
                ]),
            ),
            answer(200, &issues(Forge::GitLab, 0..most / 2)),
            answer(200, &issues(Forge::GitLab, 100..100 + second)),
        ]);
        let header = read(Forge::GitLab, &http, (false, Some(100))).unwrap();
        let rows = header.issues.as_ref().unwrap().len();
        assert_eq!(
            rows,
            MOST_ISSUE_ROWS.min(usize::try_from(most / 2 + second).unwrap())
        );
        let named: Vec<&str> = header
            .unread
            .iter()
            .map(|unread| unread.reason.as_str())
            .collect();
        let reason = format!(
            "{} open issues carry its milestones; only the first {MOST_ISSUE_ROWS} are rows",
            most / 2 + second
        );
        assert_eq!(named.contains(&reason.as_str()), over, "{named:?}");
    }
}
