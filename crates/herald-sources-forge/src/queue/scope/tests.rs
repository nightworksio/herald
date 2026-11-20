use std::collections::BTreeMap;

use herald_core::{Checks, Review, Visibility};
use herald_kinds::Queue as _;
use herald_ports::Secret;
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::*;

fn answer(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: vec![("ETag".to_owned(), "\"e\"".to_owned())],
        body: body.to_string().into_bytes(),
    }
}

fn member(id: &str, path: &str, visibility: Visibility) -> Listed {
    Listed {
        id: id.to_owned(),
        name: path.rsplit('/').next().unwrap_or(path).to_owned(),
        path: path.to_owned(),
        topics: Vec::new(),
        properties: BTreeMap::new(),
        visibility,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    }
}

fn members() -> Vec<Listed> {
    vec![
        member("1", "o/app", Visibility::Public),
        member("2", "o/vault", Visibility::Private),
    ]
}

fn scoped(forge: Forge, tokened: bool) -> Scoped {
    let options = SourceOptions {
        owner: Some("o".to_owned()),
        ..SourceOptions::default()
    };
    let owned = Owned::new(forge, ("o", None), &options, (None, 0));
    Scoped::new(
        (forge, "o"),
        &options,
        tokened.then(|| Token::Named(Secret::new("TOKEN", "t"))),
        ("https://api.github.com/graphql".to_owned(), owned),
    )
}

fn asked(owner: Owner, members: &[Listed], most: Most) -> Asked<'_> {
    Asked {
        most,
        scope: Some(Scope { owner, members }),
        stated: false,
    }
}

fn found(number: u64, repository: &str) -> Value {
    json!({
        "number": number, "title": "t", "html_url": format!("https://github.com/{repository}/pull/{number}"),
        "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
        "user": { "login": "dependabot[bot]", "type": "Bot" }, "labels": [{ "name": "deps" }],
        "repository_url": format!("https://api.github.com/repos/{repository}")
    })
}

fn searched(total: usize, items: &[Value]) -> Value {
    json!({ "total_count": total, "items": items })
}

fn urls(http: &ScriptedHttp) -> Vec<String> {
    http.asked().into_iter().map(|(url, _)| url).collect()
}

fn opened(scoped: &Scoped, http: &ScriptedHttp, asked: Asked<'_>) -> Vec<Pull> {
    pollster::block_on(scoped.opened(http, asked)).unwrap()
}

#[test]
fn github_searches_the_owner_once_keeps_its_members_rows_and_reads_them_by_one_graphql() {
    let scoped = scoped(Forge::GitHub, true);
    let members = members();
    let node = |number: u64| {
        json!({ "number": number, "title": "read", "url": "https://github.com/o/app/pull/1",
            "isDraft": false, "createdAt": "2026-11-18T10:00:00Z",
            "updatedAt": "2026-11-18T11:00:00Z", "reviewDecision": "APPROVED",
            "headRefOid": "abc" })
    };
    let http = ScriptedHttp::new(vec![
        answer(
            200,
            &searched(
                3,
                &[
                    found(1, "O/App"),
                    found(2, "o/vault"),
                    found(3, "o/other"),
                    found(5, "o/app"),
                ],
            ),
        ),
        answer(
            200,
            &json!({ "data": {
                "r0": { "visibility": "PRIVATE", "p0": node(1), "p1": node(5) },
                "r1": { "visibility": "PUBLIC", "p0": node(2) }
            } }),
        ),
    ]);
    let pulls = opened(
        &scoped,
        &http,
        asked(Owner::Organization, &members, Most::default()),
    );
    assert_eq!(
        urls(&http),
        [
            "https://api.github.com/search/issues?q=is%3Apr%20is%3Aopen%20org%3Ao&sort=updated&order=desc&per_page=100&page=1",
            "https://api.github.com/graphql",
        ]
    );
    let body = http.bodies().pop().unwrap();
    assert_eq!(
        body["variables"],
        json!({ "o0": "o", "n0": "app", "o1": "o", "n1": "vault" })
    );
    assert!(
        body["query"]
            .as_str()
            .unwrap()
            .contains("p1: pullRequest(number: 5)"),
        "{body}"
    );
    let query = body["query"].as_str().unwrap();
    assert!(
        query.contains("r1: repository(owner: $o1, name: $n1)"),
        "{query}"
    );
    let read: Vec<(u64, &str, &str, Visibility, Review)> = pulls
        .iter()
        .map(|pull| {
            (
                pull.number,
                pull.project.as_str(),
                pull.repository.as_str(),
                pull.visibility,
                pull.review,
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            (1, "app", "o/app", Visibility::Private, Review::Approved),
            (5, "app", "o/app", Visibility::Private, Review::Approved),
            (2, "vault", "o/vault", Visibility::Private, Review::Approved),
        ]
    );
}

#[test]
fn a_user_is_searched_by_user_and_a_repository_graphql_cannot_read_is_left_out() {
    let scoped = scoped(Forge::GitHub, true);
    let members = members();
    let http = ScriptedHttp::new(vec![
        answer(200, &searched(1, &[found(4, "o/app")])),
        answer(200, &json!({ "data": { "r0": null } })),
    ]);
    let pulls = opened(
        &scoped,
        &http,
        asked(Owner::User, &members, Most::default()),
    );
    assert_eq!(pulls, Vec::<Pull>::new());
    assert!(urls(&http)[0].contains("q=is%3Apr%20is%3Aopen%20user%3Ao&"));
}

#[test]
fn past_what_github_s_search_lists_each_repository_is_listed_on_its_own() {
    let scoped = scoped(Forge::GitHub, false);
    let members = members();
    let row = |number: u64, repository: &str| {
        json!({ "number": number, "title": "t",
            "html_url": format!("https://github.com/{repository}/pull/{number}"),
            "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
            "head": { "sha": "abc" } })
    };
    let http = ScriptedHttp::new(vec![
        answer(200, &searched(SEARCHED + 1, &[found(1, "o/app")])),
        answer(200, &json!([row(5, "o/app"), row(6, "o/app")])),
        answer(200, &json!([row(7, "o/vault")])),
    ]);
    let most = Most { open: 3, pulls: 0 };
    let pulls = opened(&scoped, &http, asked(Owner::Organization, &members, most));
    let read: Vec<(u64, &str, Visibility, bool)> = pulls
        .iter()
        .map(|pull| {
            (
                pull.number,
                pull.project.as_str(),
                pull.visibility,
                pull.unread,
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            (5, "app", Visibility::Public, true),
            (6, "app", Visibility::Public, true),
            (7, "vault", Visibility::Private, true),
        ]
    );
    assert_eq!(
        &urls(&http)[1..],
        [
            "https://api.github.com/repos/o/app/pulls?state=open&sort=updated&direction=desc&per_page=100&page=1",
            "https://api.github.com/repos/o/vault/pulls?state=open&sort=updated&direction=desc&per_page=100&page=1",
        ]
    );
}

#[test]
fn listing_each_repository_stops_once_it_listed_the_most() {
    let scoped = scoped(Forge::GitLab, false);
    let members = members();
    let request = json!({ "iid": 1, "title": "t", "web_url": "https://gitlab.com/o/app/-/merge_requests/1",
        "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z" });
    let http = ScriptedHttp::new(vec![answer(200, &json!([request]))]);
    let most = Most { open: 1, pulls: 1 };
    let pulls = opened(&scoped, &http, asked(Owner::User, &members, most));
    assert_eq!(pulls.len(), 1);
    assert_eq!(
        urls(&http),
        [
            "https://gitlab.com/api/v4/projects/o%2Fapp/merge_requests?state=opened&order_by=updated_at&sort=desc&per_page=100&page=1"
        ]
    );
}

#[test]
fn a_gitlab_group_lists_its_merge_requests_once_each_placed_by_its_project_id() {
    let scoped = scoped(Forge::GitLab, true);
    let members = members();
    let request = |iid: u64, project: u64| {
        json!({ "iid": iid, "title": "t", "web_url": "https://gitlab.com/o/x/-/merge_requests/1",
            "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
            "detailed_merge_status": "mergeable", "project_id": project })
    };
    let unplaced = json!({ "iid": 9, "title": "t", "web_url": "https://gitlab.com/o/x/-/merge_requests/9",
        "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z" });
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!([request(1, 2), request(2, 3), unplaced]),
    )]);
    let pulls = opened(
        &scoped,
        &http,
        asked(Owner::Organization, &members, Most::default()),
    );
    let read: Vec<(u64, &str, Checks)> = pulls
        .iter()
        .map(|pull| (pull.number, pull.repository.as_str(), pull.checks.clone()))
        .collect();
    assert_eq!(read, [(1, "o/vault", Checks::Passed)]);
    assert_eq!(
        urls(&http),
        [
            "https://gitlab.com/api/v4/groups/o/merge_requests?state=opened&scope=all&order_by=updated_at&sort=desc&per_page=100&page=1"
        ]
    );
}

#[test]
fn forgejo_searches_the_owner_reads_each_pull_request_first_and_the_rest_past_its_most_unread() {
    let scoped = scoped(Forge::Forgejo, true);
    let members = members();
    let issue = |number: u64, id: u64| {
        json!({ "number": number, "title": "t",
            "html_url": format!("https://codeberg.org/o/app/pulls/{number}"),
            "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
            "repository": { "id": id }, "pull_request": { "draft": number == 2 } })
    };
    let not_a_pull = json!({ "number": 8, "title": "t", "html_url": "https://codeberg.org/o/app/issues/8",
        "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
        "repository": { "id": 1 } });
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([issue(1, 1), issue(2, 1), not_a_pull])),
        answer(200, &json!([])),
        answer(
            200,
            &json!({ "number": 1, "title": "t", "html_url": "https://codeberg.org/o/app/pulls/1",
                "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
                "mergeable": true, "head": { "sha": "abc" },
                "requested_reviewers": [{ "login": "bob" }] }),
        ),
        answer(200, &json!([])),
        answer(
            200,
            &json!({ "statuses": [{ "context": "ci", "status": "success" }] }),
        ),
    ]);
    let most = Most {
        open: 3 * PAGE,
        pulls: 1,
    };
    let pulls = opened(&scoped, &http, asked(Owner::Organization, &members, most));
    assert_eq!(
        urls(&http),
        [
            "https://codeberg.org/api/v1/repos/issues/search?type=pulls&state=open&owner=o&limit=100&page=1",
            "https://codeberg.org/api/v1/repos/issues/search?type=pulls&state=open&owner=o&limit=100&page=2",
            "https://codeberg.org/api/v1/repos/o/app/pulls/1",
            "https://codeberg.org/api/v1/repos/o/app/pulls/1/reviews?limit=100",
            "https://codeberg.org/api/v1/repos/o/app/commits/abc/status?limit=100",
        ]
    );
    let first = pulls.first().unwrap();
    assert_eq!(
        (
            first.head.as_deref(),
            first.review,
            first.checks.clone(),
            first.reviewers.clone()
        ),
        (
            Some("abc"),
            Review::Requested,
            Checks::Passed,
            vec!["bob".to_owned()]
        )
    );
    let second = pulls.get(1).unwrap();
    assert_eq!((second.draft, second.unread), (true, true));
}

#[test]
fn a_kept_listing_asks_each_page_conditionally_and_finds_it_unchanged() {
    let scoped = scoped(Forge::GitHub, true);
    let members = members();
    let kept = [Validators {
        etag: Some("\"e\"".to_owned()),
        last_modified: None,
    }];
    let http = ScriptedHttp::new(vec![Response {
        status: 304,
        headers: Vec::new(),
        body: Vec::new(),
    }]);
    let listed = pollster::block_on(scoped.listed(
        &http,
        asked(Owner::Organization, &members, Most::default()),
        &kept,
    ))
    .unwrap();
    assert_eq!(listed, Relisted::Unchanged);
    assert_eq!(http.asked()[0].1.as_deref(), Some("\"e\""));
    let http = ScriptedHttp::new(vec![answer(200, &searched(1, &[found(1, "o/app")]))]);
    let listed = pollster::block_on(scoped.listed(
        &http,
        asked(Owner::Organization, &members, Most::default()),
        &[],
    ))
    .unwrap();
    let Relisted::Listed {
        pulls, validators, ..
    } = listed
    else {
        panic!("{listed:?}");
    };
    assert_eq!(pulls.first().unwrap().head, None);
    assert_eq!(validators.first().unwrap().etag.as_deref(), Some("\"e\""));
}

#[test]
fn asked_without_its_scope_it_reads_nothing_and_says_it_is_scoped() {
    let scoped = scoped(Forge::GitHub, true);
    let http = ScriptedHttp::new(Vec::new());
    assert_eq!(opened(&scoped, &http, Asked::default()), Vec::<Pull>::new());
    let listed = pollster::block_on(scoped.listed(&http, Asked::default(), &[])).unwrap();
    assert_eq!(listed, Relisted::Unchanged);
    assert_eq!(http.asked().len(), 0);
    assert!(scoped.scoped());
}

#[test]
fn a_kept_completion_reads_each_changed_pull_request_through_its_own_repository() {
    let scoped = scoped(Forge::GitLab, true);
    let pull = Pull {
        project: "app".to_owned(),
        repository: "o/app".to_owned(),
        visibility: Visibility::Public,
        number: 1,
        title: "t".to_owned(),
        url: "https://gitlab.com/o/app/-/merge_requests/1".to_owned(),
        author: None,
        bot: false,
        draft: false,
        labels: Vec::new(),
        opened: "2026-11-18T10:00:00Z".parse().unwrap(),
        updated: "2026-11-18T11:00:00Z".parse().unwrap(),
        review: Review::Unasked,
        reviewers: Vec::new(),
        checks: Checks::Absent,
        mergeable: herald_core::Mergeable::Unknown,
        queued: false,
        forge: None,
        head: None,
        unread: false,
    };
    let http = ScriptedHttp::new(Vec::new());
    let read =
        pollster::block_on(scoped.completed(&http, Asked::default(), vec![pull.clone()])).unwrap();
    assert_eq!(read, [pull]);
}

#[test]
fn a_failed_scope_query_fails_the_read() {
    let scoped = scoped(Forge::GitHub, true);
    let members = members();
    let http = ScriptedHttp::new(vec![answer(500, &json!({}))]);
    let failed = pollster::block_on(
        scoped.opened(&http, asked(Owner::Organization, &members, Most::default())),
    );
    assert!(
        matches!(&failed, Err(Unpolled::Unreadable(why)) if why.starts_with(search::GITHUB)),
        "{failed:?}"
    );
}

#[test]
fn a_scope_query_costs_its_pages_and_what_reads_its_pull_requests_in_full() {
    let most = Most {
        open: 1500,
        pulls: 20,
    };
    let costs: Vec<(&str, usize, bool, bool, bool)> = [
        (Forge::GitHub, true),
        (Forge::GitHub, false),
        (Forge::GitLab, true),
        (Forge::Forgejo, false),
    ]
    .into_iter()
    .map(|(forge, tokened)| {
        let cost = scope_cost(forge, tokened, most);
        (
            cost.path,
            cost.requests,
            cost.detailed,
            cost.mergeability,
            cost.scoped,
        )
    })
    .collect();
    assert_eq!(
        costs,
        [
            ("search", 20, false, true, true),
            ("search", 70, true, false, true),
            ("group listing", 15, false, true, true),
            ("issue search", 75, true, true, true),
        ]
    );
}

#[test]
fn a_search_finding_as_many_as_it_lists_is_not_listed_repository_by_repository() {
    let scoped = scoped(Forge::GitHub, true);
    let members = members();
    let http = ScriptedHttp::new(vec![
        answer(200, &searched(SEARCHED, &[found(1, "o/app")])),
        answer(
            200,
            &json!({ "data": { "r0": { "visibility": "PUBLIC" } } }),
        ),
    ]);
    opened(
        &scoped,
        &http,
        asked(Owner::Organization, &members, Most::default()),
    );
    assert_eq!(urls(&http).len(), 2);
}

#[test]
fn a_full_page_of_a_group_s_merge_requests_is_followed_by_the_next() {
    let scoped = scoped(Forge::GitLab, true);
    let members = members();
    let page: Vec<Value> = (1..=PAGE as u64)
        .map(|iid| {
            json!({ "iid": iid, "title": "t", "web_url": "https://gitlab.com/o/app/-/merge_requests/1",
                "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
                "project_id": 1 })
        })
        .collect();
    let http = ScriptedHttp::new(vec![answer(200, &json!(page)), answer(200, &json!([]))]);
    let pulls = opened(
        &scoped,
        &http,
        asked(Owner::Organization, &members, Most::default()),
    );
    assert_eq!(pulls.len(), PAGE);
    assert_eq!(urls(&http).len(), 2);
}

#[test]
fn a_query_lists_at_most_what_its_forge_s_query_lists() {
    assert_eq!(queried(Forge::GitHub, SEARCHED + 1), SEARCHED);
    assert_eq!(queried(Forge::GitHub, SEARCHED), SEARCHED);
    assert_eq!(queried(Forge::GitLab, SEARCHED + 1), SEARCHED + 1);
    assert_eq!(queried(Forge::Forgejo, SEARCHED + 1), SEARCHED + 1);
}

/// An open pull request `#1` of the repository at `path`, read from a
/// listing.
fn pull_of(path: &str) -> Pull {
    Pull {
        project: "app".to_owned(),
        repository: path.to_owned(),
        visibility: Visibility::Private,
        number: 1,
        title: "t".to_owned(),
        url: format!("https://codeberg.org/{path}/pulls/1"),
        author: None,
        bot: false,
        draft: false,
        labels: Vec::new(),
        opened: "2026-11-18T10:00:00Z".parse().unwrap(),
        updated: "2026-11-18T11:00:00Z".parse().unwrap(),
        review: Review::Unasked,
        reviewers: Vec::new(),
        checks: Checks::Absent,
        mergeable: herald_core::Mergeable::Unknown,
        queued: false,
        forge: None,
        head: None,
        unread: false,
    }
}

#[test]
fn a_kept_completion_of_a_pull_request_whose_head_is_known_reads_only_its_reviews_and_checks() {
    let scoped = scoped(Forge::Forgejo, true);
    let mut pull = Pull {
        head: Some("abc".to_owned()),
        ..pull_of("o/app")
    };
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([])),
        answer(
            200,
            &json!({ "statuses": [{ "context": "ci", "status": "success" }] }),
        ),
    ]);
    let read =
        pollster::block_on(scoped.completed(&http, Asked::default(), vec![pull.clone()])).unwrap();
    assert_eq!(
        urls(&http),
        [
            "https://codeberg.org/api/v1/repos/o/app/pulls/1/reviews?limit=100",
            "https://codeberg.org/api/v1/repos/o/app/commits/abc/status?limit=100",
        ]
    );
    pull.checks = Checks::Passed;
    assert_eq!(read[0].checks, pull.checks);
    assert_eq!(read[0].head, pull.head);
}

#[test]
fn a_placed_row_is_seen_as_its_member_is_whatever_the_listing_said() {
    let scoped = scoped(Forge::GitHub, false);
    let members = members();
    let listed = |visibility: Visibility, path: &str| Pull {
        visibility,
        ..pull_of(path)
    };
    for (said, path, seen) in [
        (Visibility::Private, "o/app", Visibility::Public),
        (Visibility::Public, "o/vault", Visibility::Private),
    ] {
        let placed = scoped
            .placed(
                &members,
                &Located::Path(path.to_owned()),
                listed(said, path),
            )
            .unwrap();
        assert_eq!(
            (placed.repository.as_str(), placed.visibility),
            (path, seen)
        );
    }
}

#[test]
fn github_s_search_is_asked_for_no_more_than_it_lists() {
    assert_eq!(queried(Forge::GitHub, 5), 5);
    assert_eq!(queried(Forge::GitHub, SEARCHED), SEARCHED);
    assert_eq!(queried(Forge::GitHub, SEARCHED + 1), SEARCHED);
    assert_eq!(queried(Forge::GitLab, SEARCHED + 1), SEARCHED + 1);
}
