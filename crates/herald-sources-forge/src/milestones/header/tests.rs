use herald_core::{Glob, Visibility};
use herald_ports::Response;
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::*;
use crate::SourceOptions;

const NOW: &str = "2026-11-23T12:00:00Z";

const HEADED: HeaderRead = HeaderRead {
    header: true,
    issues: false,
};

fn answer(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

fn github(number: u64, title: &str, due: &str) -> Value {
    json!({
        "id": 1000 + number, "number": number, "title": title, "description": null,
        "state": "open", "open_issues": 2, "closed_issues": 12,
        "created_at": "2026-11-18T08:00:00Z", "updated_at": "2026-11-18T10:00:00Z",
        "closed_at": null, "due_on": format!("{due}T08:00:00Z"),
        "html_url": format!("https://github.com/o/r/milestone/{number}"),
    })
}

fn gitlab(id: u64, title: &str) -> Value {
    json!({
        "id": id, "iid": id + 100, "title": title, "description": null, "state": "active",
        "project_id": 7, "group_id": null, "due_date": "2026-11-22",
        "created_at": "2026-11-18T08:00:00Z", "updated_at": "2026-11-18T10:00:00Z",
        "web_url": format!("https://gitlab.com/o/r/-/milestones/{}", id + 100),
    })
}

fn headed(group: Option<&str>) -> Headed {
    Headed {
        source: 0,
        path: group.unwrap_or("o/r").to_owned(),
        visibility: Visibility::Internal,
        picked: false,
        group: group.is_some(),
    }
}

fn read(
    forge: Forge,
    http: &ScriptedHttp,
    (glob, page): (&str, Option<usize>),
) -> Result<Header, Unpolled> {
    read_as(forge, http, (glob, page), &headed(None))
}

fn read_as(
    forge: Forge,
    http: &ScriptedHttp,
    (glob, page): (&str, Option<usize>),
    headed: &Headed,
) -> Result<Header, Unpolled> {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        page: page.and_then(std::num::NonZeroUsize::new),
        ..SourceOptions::default()
    };
    let origin = Origin::new(forge, &options, "o/r");
    let glob = Glob::new(glob).unwrap();
    let http = Erased(http as &dyn DynHttp);
    pollster::block_on(header(
        &http,
        (&origin, None),
        (headed, &glob),
        (HEADED, NOW.parse().unwrap()),
    ))
}

fn urls(http: &ScriptedHttp) -> Vec<String> {
    http.asked().into_iter().map(|(url, _)| url).collect()
}

#[test]
fn a_github_header_names_each_open_milestone_the_glob_matches_from_one_listing() {
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!([
            github(1, "v2.0", "2026-11-22"),
            github(2, "v2.1", "2026-11-23"),
            github(3, "v3.0", "2026-11-01"),
        ]),
    )]);
    let header = read(Forge::GitHub, &http, ("v2.*", None)).unwrap();
    assert_eq!(
        urls(&http),
        ["https://api.github.com/repos/o/r/milestones?state=open&per_page=30"]
    );
    assert!(header.unread.is_empty(), "{header:?}");
    let first = &header.milestones[0];
    assert_eq!(first.repository, "o/r");
    assert_eq!(first.visibility, Visibility::Internal);
    assert_eq!(first.title, "v2.0");
    assert_eq!(
        first.url.as_deref(),
        Some("https://github.com/o/r/milestone/1")
    );
    assert_eq!(first.due, Some("2026-11-22".parse().unwrap()));
    assert!(first.overdue);
    assert_eq!(first.counts, Some((2, 12)));
    let second = &header.milestones[1];
    assert_eq!(second.title, "v2.1");
    assert_eq!(second.due, Some("2026-11-23".parse().unwrap()));
    assert!(!second.overdue);
    assert_eq!(header.milestones.len(), 2);
}

#[test]
fn a_gitlab_header_counts_each_milestone_s_issues_by_one_more_request() {
    let statistics = json!({ "statistics": { "counts": { "all": 5, "closed": 4, "opened": 1 } } });
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([gitlab(1, "v2.0"), gitlab(2, "v1.0")])),
        answer(200, &statistics),
    ]);
    let header = read(Forge::GitLab, &http, ("v2.*", None)).unwrap();
    assert_eq!(
        urls(&http),
        [
            "https://gitlab.com/api/v4/projects/o%2Fr/milestones?state=active&per_page=30",
            "https://gitlab.com/api/v4/projects/o%2Fr/issues_statistics?milestone=v2.0",
        ]
    );
    assert_eq!(header.milestones.len(), 1);
    assert_eq!(header.milestones[0].counts, Some((1, 4)));
    assert!(header.milestones[0].overdue);
}

#[test]
fn a_forgejo_header_lists_by_its_limit() {
    let http = ScriptedHttp::new(vec![answer(200, &json!([github(1, "v2.0", "2026-12-01")]))]);
    let header = read(Forge::Forgejo, &http, ("v2.0", None)).unwrap();
    assert!(urls(&http)[0].ends_with("/repos/o/r/milestones?state=open&limit=30"));
    assert_eq!(header.milestones.len(), 1);
    assert!(!header.milestones[0].overdue);
}

#[test]
fn a_refused_or_unseen_listing_names_the_repository_unread_with_why() {
    for (forge, status, reason) in [
        (Forge::GitHub, 403, GITHUB_REFUSED),
        (Forge::GitHub, 401, GITHUB_REFUSED),
        (Forge::GitLab, 403, GITLAB_REFUSED),
        (Forge::GitLab, 401, GITLAB_REFUSED),
        (Forge::Forgejo, 403, FORGEJO_REFUSED),
        (Forge::Forgejo, 401, FORGEJO_REFUSED),
        (Forge::GitHub, 404, UNSEEN),
        (Forge::Forgejo, 404, UNSEEN),
    ] {
        let http = ScriptedHttp::new(vec![answer(status, &json!({ "message": "no" }))]);
        let header = read(forge, &http, ("*", None)).unwrap();
        assert_eq!(header.milestones, []);
        assert_eq!(header.unread.len(), 1, "{forge:?} {status}");
        assert_eq!(header.unread[0].reason, reason, "{forge:?} {status}");
        assert_eq!(header.unread[0].repository, "o/r");
        assert_eq!(header.unread[0].visibility, Visibility::Internal);
    }
}

#[test]
fn a_rate_limit_or_a_failed_answer_stops_the_read() {
    let limited = Response {
        status: 403,
        headers: vec![("x-ratelimit-remaining".to_owned(), "0".to_owned())],
        body: b"{\"message\":\"no\"}".to_vec(),
    };
    let http = ScriptedHttp::new(vec![limited]);
    assert!(matches!(
        read(Forge::GitHub, &http, ("*", None)),
        Err(Unpolled::RateLimited(_))
    ));
    let http = ScriptedHttp::new(vec![answer(429, &json!({}))]);
    assert!(matches!(
        read(Forge::Forgejo, &http, ("*", None)),
        Err(Unpolled::RateLimited(_))
    ));
    let http = ScriptedHttp::new(vec![answer(500, &json!({}))]);
    assert!(matches!(
        read(Forge::GitHub, &http, ("*", None)),
        Err(Unpolled::Unreadable(_))
    ));
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([gitlab(1, "v2.0")])),
        answer(429, &json!({})),
    ]);
    assert!(matches!(
        read(Forge::GitLab, &http, ("*", None)),
        Err(Unpolled::RateLimited(_))
    ));
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([gitlab(1, "v2.0")])),
        answer(403, &json!({})),
    ]);
    assert!(matches!(
        read(Forge::GitLab, &http, ("*", None)),
        Err(Unpolled::Unreadable(_))
    ));
}

#[test]
fn a_full_page_is_named_as_holding_more_than_it_matched() {
    let two = json!([github(1, "v1", "2026-12-01"), github(2, "v2", "2026-12-01")]);
    let http = ScriptedHttp::new(vec![answer(200, &two)]);
    let header = read(Forge::GitHub, &http, ("*", Some(2))).unwrap();
    assert!(urls(&http)[0].ends_with("per_page=2"));
    assert_eq!(header.milestones.len(), 2);
    assert_eq!(
        header.unread[0].reason,
        "its listing held more than 2 open milestones; only those 2 are matched"
    );
    let http = ScriptedHttp::new(vec![answer(200, &two)]);
    let header = read(Forge::GitHub, &http, ("*", Some(3))).unwrap();
    assert!(header.unread.is_empty(), "{header:?}");
    let http = ScriptedHttp::new(vec![answer(200, &two)]);
    let header = read(Forge::GitHub, &http, ("*", Some(1))).unwrap();
    assert_eq!(header.unread.len(), 1, "{header:?}");
}

#[test]
fn past_the_most_milestones_a_repository_s_header_names_it_counts_none_more() {
    let listing = |count: u64| {
        Value::from(
            (1..=count)
                .map(|number| github(number, &format!("v{number}"), "2026-12-01"))
                .collect::<Vec<_>>(),
        )
    };
    let most = MOST_HEADER_MILESTONES as u64;
    let http = ScriptedHttp::new(vec![answer(200, &listing(most))]);
    let header = read(Forge::GitHub, &http, ("*", None)).unwrap();
    assert_eq!(header.milestones.len(), MOST_HEADER_MILESTONES);
    assert!(header.unread.is_empty(), "{header:?}");
    let http = ScriptedHttp::new(vec![answer(200, &listing(most + 1))]);
    let header = read(Forge::GitHub, &http, ("*", None)).unwrap();
    assert_eq!(header.milestones.len(), MOST_HEADER_MILESTONES);
    assert_eq!(
        header.unread[0].reason,
        format!("11 open milestones match; only the first {MOST_HEADER_MILESTONES} are named")
    );
    assert_eq!(header.milestones[9].title, "v10");
}

#[test]
fn a_gitlab_group_that_refuses_is_named_unread_as_the_group() {
    let http = ScriptedHttp::new(vec![answer(403, &json!({}))]);
    let header = read_as(Forge::GitLab, &http, ("v2.*", None), &headed(Some("o/sub"))).unwrap();
    assert_eq!(
        urls(&http),
        ["https://gitlab.com/api/v4/groups/o%2Fsub/milestones?state=active&per_page=30"]
    );
    let unread: Vec<(&str, Visibility, &str)> = header
        .unread
        .iter()
        .map(|unread| {
            (
                unread.repository.as_str(),
                unread.visibility,
                unread.reason.as_str(),
            )
        })
        .collect();
    assert_eq!(unread, [("o/sub", Visibility::Internal, GITLAB_REFUSED)]);
}
