use herald_core::{Advising, Advisories, AdvisoriesFrom, Listed, Owner, Visibility};
use herald_kinds::{Asked, Scope};
use herald_ports::{Response, Secret};
use herald_testing::ScriptedHttp;
use jiff::Timestamp;
use serde_json::{Value, json};

use super::*;
use crate::queue::{Queued, Scoped};
use crate::{Origin, SourceOptions, Token};

const SECRET_VALUE: &str = "never-shown-value";

fn answer(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

fn paged(body: &Value, next: &str) -> Response {
    Response {
        headers: vec![(
            "Link".to_owned(),
            format!("<{next}>; rel=\"next\", <https://api.github.com/x?page=9>; rel=\"last\""),
        )],
        ..answer(200, body)
    }
}

fn at() -> Timestamp {
    "2026-11-20T10:00:00Z".parse().unwrap()
}

fn advising(from: AdvisoriesFrom, (repositories, most): (usize, usize)) -> (Advising, Timestamp) {
    (
        Advising {
            from,
            repositories,
            most,
        },
        at(),
    )
}

fn queued(forge: Forge) -> Queued {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        ..SourceOptions::default()
    };
    Queued {
        origin: Origin::new(forge, &options, "o/r"),
        token: Some(Token::Named(Secret::new("TOKEN", "t"))),
        project: "r".to_owned(),
        graphql: "https://api.github.com/graphql".to_owned(),
    }
}

fn scoped(forge: Forge) -> Scoped {
    let options = SourceOptions {
        owner: Some("o".to_owned()),
        ..SourceOptions::default()
    };
    let owned = crate::owner::Owned::new(forge, ("o", None), &options, (None, 0));
    Scoped::new(
        (forge, "o"),
        &options,
        None,
        ("https://api.github.com/graphql".to_owned(), owned),
    )
}

fn member(path: &str) -> Listed {
    Listed {
        id: path.to_owned(),
        name: path.rsplit('/').next().unwrap_or(path).to_owned(),
        path: path.to_owned(),
        topics: Vec::new(),
        properties: std::collections::BTreeMap::new(),
        visibility: Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
        linked: None,
    }
}

fn dependabot(number: u64, repository: Option<&str>) -> Value {
    json!({
        "number": number, "state": "open",
        "html_url": format!("https://github.com/o/r/security/dependabot/{number}"),
        "created_at": "2026-11-18T10:00:00Z",
        "security_advisory": { "ghsa_id": "GHSA-1", "summary": "Prototype pollution", "severity": "high" },
        "dependency": { "package": { "name": "left-pad", "ecosystem": "npm" }, "manifest_path": "package.json" },
        "repository": repository.map(|name| json!({ "full_name": name })),
    })
}

fn secret(number: u64) -> Value {
    json!({
        "number": number, "state": "open",
        "html_url": format!("https://github.com/o/r/security/secret-scanning/{number}"),
        "created_at": "2026-11-19T10:00:00Z",
        "secret_type": "example_service_key", "secret_type_display_name": "Example Service Key",
        "secret": SECRET_VALUE,
        "most_recent_instance": { "location": { "path": "config/keys.yml" } },
    })
}

fn read(
    queue: &dyn herald_kinds::Queue,
    answers: Vec<Response>,
    (asked, shown, advising): (Asked<'_>, &[&str], (Advising, Timestamp)),
) -> (Advisories, Vec<(String, Value)>) {
    let http = ScriptedHttp::new(answers);
    let found = pollster::block_on(queue.advisories(&http, (asked, shown), advising)).unwrap();
    let urls = http.asked().into_iter().map(|(url, _)| url);
    (found, urls.zip(http.bodies()).collect())
}

#[test]
fn a_github_repository_s_three_listings_are_read_page_by_page_and_a_refusal_named() {
    let (found, asked) = read(
        &queued(Forge::GitHub),
        vec![
            answer(200, &json!([])),
            paged(
                &json!([dependabot(1, None)]),
                "https://api.github.com/repos/o/r/dependabot/alerts?after=c1",
            ),
            answer(200, &json!([dependabot(2, None)])),
            answer(
                403,
                &json!({ "message": "Advanced Security must be enabled" }),
            ),
        ],
        (
            Asked::default(),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 1000)),
        ),
    );
    let urls: Vec<&str> = asked.iter().map(|(url, _)| url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "https://api.github.com/repos/o/r/code-scanning/alerts?state=open&per_page=100",
            "https://api.github.com/repos/o/r/dependabot/alerts?state=open&per_page=100",
            "https://api.github.com/repos/o/r/dependabot/alerts?after=c1",
            "https://api.github.com/repos/o/r/secret-scanning/alerts?state=open&per_page=100&hide_secret=true",
        ]
    );
    assert_eq!(found.open.len(), 2);
    let first = &found.open[0];
    assert_eq!(first.repository, "o/r");
    assert_eq!(first.tool, "dependency-scanning");
    assert_eq!(first.severity.as_deref(), Some("high"));
    assert_eq!(first.summary.as_deref(), Some("Prototype pollution"));
    assert_eq!(first.state, "open");
    assert!(!found.more);
    assert_eq!(found.at, at());
    assert_eq!(found.unread.len(), 1);
    assert_eq!(
        found.unread[0].reason,
        format!("GitHub's secret scanning alerts: {GITHUB_REFUSED}")
    );
    assert_eq!(found.unread[0].repository, "o/r");
}

#[test]
fn a_secret_s_row_names_its_type_repository_state_age_and_link_alone() {
    let (found, _) = read(
        &queued(Forge::GitHub),
        vec![
            answer(200, &json!([])),
            answer(200, &json!([])),
            answer(200, &json!([secret(4)])),
        ],
        (
            Asked::default(),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 1000)),
        ),
    );
    let row = &found.open[0];
    assert_eq!(row.tool, "secret-scanning");
    assert_eq!(row.summary.as_deref(), Some("Example Service Key"));
    assert_eq!(row.severity, None);
    assert_eq!(row.url, "https://github.com/o/r/security/secret-scanning/4");
    let shown = format!("{found:?}");
    assert!(!shown.contains(SECRET_VALUE), "{shown}");
    assert!(!shown.contains("keys.yml"), "{shown}");
    assert!(!shown.contains("example_service_key"), "{shown}");
}

#[test]
fn a_listing_stops_at_the_most_and_at_a_page_off_its_api_saying_more_were_open() {
    let (found, asked) = read(
        &queued(Forge::GitHub),
        vec![answer(
            200,
            &json!([dependabot(1, None), dependabot(2, None)]),
        )],
        (
            Asked::default(),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 1)),
        ),
    );
    assert_eq!(found.open.len(), 1);
    assert!(found.more);
    assert_eq!(asked.len(), 1);
    let (exact, asked) = read(
        &queued(Forge::GitHub),
        vec![paged(
            &json!([dependabot(1, None)]),
            "https://api.github.com/repos/o/r/code-scanning/alerts?page=2",
        )],
        (
            Asked::default(),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 1)),
        ),
    );
    assert!(exact.more);
    assert_eq!(asked.len(), 1);
    let (elsewhere, asked) = read(
        &queued(Forge::GitHub),
        vec![
            paged(
                &json!([dependabot(1, None)]),
                "https://elsewhere.example/repos/o/r/code-scanning/alerts?page=2",
            ),
            answer(200, &json!([])),
            answer(200, &json!([])),
        ],
        (
            Asked::default(),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 10)),
        ),
    );
    assert!(elsewhere.more);
    assert!(
        asked
            .iter()
            .all(|(url, _)| url.starts_with("https://api.github.com/"))
    );
    assert_eq!(elsewhere.open.len(), 1);
    let (unseen, _) = read(
        &queued(Forge::GitHub),
        vec![
            answer(404, &json!({})),
            answer(200, &json!([])),
            answer(200, &json!([])),
        ],
        (
            Asked::default(),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 10)),
        ),
    );
    assert_eq!(
        unseen.unread[0].reason,
        format!("GitHub's code scanning alerts: {GITHUB_UNSEEN}")
    );
}

#[test]
fn a_repository_s_listing_that_fills_its_most_with_a_page_left_stops_saying_more() {
    let alerts: Vec<Value> = (1..=150).map(|number| dependabot(number, None)).collect();
    let (found, asked) = read(
        &queued(Forge::GitHub),
        vec![
            paged(
                &json!(alerts),
                "https://api.github.com/repos/o/r/dependabot/alerts?page=2",
            ),
            answer(200, &json!([dependabot(151, None)])),
            answer(200, &json!([])),
            answer(200, &json!([])),
        ],
        (
            Asked::default(),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 150)),
        ),
    );
    assert_eq!((found.open.len(), found.more), (150, true));
    assert_eq!(asked.len(), 1);
}

#[test]
fn a_listing_reads_no_more_pages_than_its_most_can_fill() {
    let (capped, asked) = read(
        &queued(Forge::GitHub),
        vec![
            paged(
                &json!([dependabot(1, None)]),
                "https://api.github.com/repos/o/r/code-scanning/alerts?page=2",
            ),
            answer(200, &json!([])),
            answer(200, &json!([])),
        ],
        (
            Asked::default(),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 100)),
        ),
    );
    assert!(capped.more);
    assert_eq!(asked.len(), 3);
    let (gitlab, asked) = read(
        &queued(Forge::GitLab),
        vec![answer(
            200,
            &vulnerabilities("project", &[vulnerability(1, "DETECTED", None)], Some("c1")),
        )],
        (
            Asked::default(),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 100)),
        ),
    );
    assert!(gitlab.more);
    assert_eq!(asked.len(), 1);
    let (exact, _) = read(
        &queued(Forge::GitLab),
        vec![answer(
            200,
            &vulnerabilities("project", &[vulnerability(1, "DETECTED", None)], None),
        )],
        (
            Asked::default(),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 1)),
        ),
    );
    assert_eq!(exact.open.len(), 1);
    assert!(!exact.more);
}

fn vulnerability(id: u64, state: &str, project: Option<&str>) -> Value {
    json!({
        "webUrl": format!("https://gitlab.com/o/r/-/security/vulnerabilities/{id}"),
        "title": "SQL injection", "state": state, "severity": "CRITICAL", "reportType": "SAST",
        "detectedAt": "2026-11-18T10:00:00Z", "confirmedAt": "2026-11-19T10:00:00Z",
        "location": { "file": "app/db.rb" },
        "project": project.map(|path| json!({ "fullPath": path })),
    })
}

fn vulnerabilities(holder: &str, nodes: &[Value], next: Option<&str>) -> Value {
    json!({ "data": { holder: { "vulnerabilities": {
        "nodes": nodes,
        "pageInfo": { "hasNextPage": next.is_some(), "endCursor": next },
    } } } })
}

#[test]
fn a_gitlab_project_s_open_vulnerabilities_are_paged_by_cursor() {
    let (found, asked) = read(
        &queued(Forge::GitLab),
        vec![
            answer(
                200,
                &vulnerabilities("project", &[vulnerability(1, "DETECTED", None)], Some("c1")),
            ),
            answer(
                200,
                &vulnerabilities("project", &[vulnerability(2, "CONFIRMED", None)], None),
            ),
        ],
        (
            Asked::default(),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 1000)),
        ),
    );
    assert_eq!(asked[0].0, "https://gitlab.com/api/graphql");
    let first: Value = asked[0].1.clone();
    assert_eq!(first["variables"]["path"], "o/r");
    assert_eq!(first["variables"]["after"], Value::Null);
    assert!(
        first["query"]
            .as_str()
            .unwrap()
            .contains("project(fullPath")
    );
    assert!(
        first["query"]
            .as_str()
            .unwrap()
            .contains("state: [DETECTED, CONFIRMED]")
    );
    let second: Value = asked[1].1.clone();
    assert_eq!(second["variables"]["after"], "c1");
    let states: Vec<&str> = found.open.iter().map(|row| row.state.as_str()).collect();
    assert_eq!(states, ["open", "confirmed"]);
    assert_eq!(found.open[0].tool, "code-scanning");
    assert_eq!(found.open[0].severity.as_deref(), Some("critical"));
    assert_eq!(found.unread, []);
}

#[test]
fn gitlab_answering_no_vulnerabilities_or_refusing_reads_as_unread_never_clean() {
    for refusal in [
        answer(
            200,
            &json!({ "data": { "project": { "vulnerabilities": null } } }),
        ),
        answer(401, &json!({})),
    ] {
        let (found, _) = read(
            &queued(Forge::GitLab),
            vec![refusal],
            (
                Asked::default(),
                &[],
                advising(AdvisoriesFrom::Owner, (20, 1000)),
            ),
        );
        assert_eq!(found.open, []);
        assert_eq!(found.unread[0].reason, GITLAB_REFUSED);
    }
}

#[test]
fn forgejo_reads_as_unread_with_the_reason_and_asks_nothing() {
    for queue in [
        &queued(Forge::Forgejo) as &dyn herald_kinds::Queue,
        &scoped(Forge::Forgejo),
    ] {
        let members = [member("o/app")];
        let asked = Asked {
            scope: Some(Scope {
                owner: Owner::Organization,
                members: &members,
                narrowed: false,
            }),
            ..Asked::default()
        };
        let (found, urls) = read(
            queue,
            Vec::new(),
            (
                asked,
                &["o/app"],
                advising(AdvisoriesFrom::Shown, (20, 1000)),
            ),
        );
        assert_eq!(urls, []);
        assert_eq!(found.unread.len(), 1);
        assert_eq!(found.unread[0].reason, FORGEJO_UNRAISED);
    }
}

#[test]
fn an_organization_s_listings_keep_only_the_members_its_record_holds() {
    let members = [member("o/app"), member("o/lib")];
    let asked = Asked {
        scope: Some(Scope {
            owner: Owner::Organization,
            members: &members,
            narrowed: false,
        }),
        ..Asked::default()
    };
    let (found, urls) = read(
        &scoped(Forge::GitHub),
        vec![
            answer(403, &json!({})),
            answer(
                200,
                &json!([
                    dependabot(1, Some("O/App")),
                    dependabot(2, Some("o/outside"))
                ]),
            ),
            answer(200, &json!([])),
        ],
        (asked, &[], advising(AdvisoriesFrom::Owner, (20, 1000))),
    );
    assert_eq!(
        urls[0].0,
        "https://api.github.com/orgs/o/code-scanning/alerts?state=open&per_page=100"
    );
    assert_eq!(found.open.len(), 1);
    assert_eq!(found.open[0].repository, "o/app");
    assert_eq!(found.unread[0].repository, "o");
    assert_eq!(
        found.unread[0].reason,
        format!("GitHub's code scanning alerts: {GITHUB_ORGANIZATION_REFUSED}")
    );
    let (group, urls) = read(
        &scoped(Forge::GitLab),
        vec![answer(
            200,
            &vulnerabilities(
                "group",
                &[
                    vulnerability(1, "DETECTED", Some("o/lib")),
                    vulnerability(2, "DETECTED", None),
                ],
                None,
            ),
        )],
        (asked, &[], advising(AdvisoriesFrom::Owner, (20, 1000))),
    );
    let body: Value = urls[0].1.clone();
    assert!(body["query"].as_str().unwrap().contains("group(fullPath"));
    assert!(
        body["query"]
            .as_str()
            .unwrap()
            .contains("project { fullPath }")
    );
    assert_eq!(group.open.len(), 1);
    assert_eq!(group.open[0].repository, "o/lib");
}

#[test]
fn a_user_s_or_the_shown_members_are_read_one_by_one_within_the_bounds() {
    let members = [member("o/app"), member("o/lib")];
    let empty = || answer(200, &json!([]));
    let user = Asked {
        scope: Some(Scope {
            owner: Owner::User,
            members: &members,
            narrowed: false,
        }),
        ..Asked::default()
    };
    let (found, urls) = read(
        &scoped(Forge::GitHub),
        vec![
            answer(
                200,
                &json!([{ "full_name": "o/lib" }, { "full_name": "o/app" }]),
            ),
            answer(200, &json!([dependabot(1, None)])),
            empty(),
            empty(),
        ],
        (user, &[], advising(AdvisoriesFrom::Owner, (2, 1))),
    );
    assert_eq!(
        urls[0].0,
        "https://api.github.com/users/o/repos?sort=pushed&direction=desc&per_page=2"
    );
    assert!(
        urls[1].0.contains("/repos/o/lib/code-scanning/"),
        "{urls:?}"
    );
    assert_eq!(found.open[0].repository, "o/lib");
    assert!(found.more);
    assert_eq!(urls.len(), 2);
    let organization = Asked {
        scope: Some(Scope {
            owner: Owner::Organization,
            members: &members,
            narrowed: false,
        }),
        ..Asked::default()
    };
    let (shown, urls) = read(
        &scoped(Forge::GitHub),
        vec![empty(), empty(), empty()],
        (
            organization,
            &["o/app"],
            advising(AdvisoriesFrom::Shown, (20, 1000)),
        ),
    );
    assert!(shown.open.is_empty() && !shown.more);
    assert_eq!(
        shown.unread[0].reason,
        "1 of its member repositories were not read: only those the view shows an open pull request of are"
    );
    assert_eq!(urls.len(), 3);
    assert!(urls.iter().all(|(url, _)| url.contains("/repos/o/app/")));
}

#[test]
fn an_organization_s_newest_pushed_members_are_read_and_the_rest_named_unread() {
    let members = [member("o/app"), member("o/lib")];
    let empty = || answer(200, &json!([]));
    let organization = Asked {
        scope: Some(Scope {
            owner: Owner::Organization,
            members: &members,
            narrowed: false,
        }),
        ..Asked::default()
    };
    let (members_read, urls) = read(
        &scoped(Forge::GitHub),
        vec![
            answer(200, &json!([{ "full_name": "o/app" }])),
            empty(),
            empty(),
            empty(),
        ],
        (
            organization,
            &[],
            advising(AdvisoriesFrom::Members, (1, 1000)),
        ),
    );
    assert_eq!(members_read.open, []);
    assert!(members_read.more);
    assert_eq!(
        (
            members_read.unread[0].repository.as_str(),
            members_read.unread[0].reason.as_str()
        ),
        (
            "o",
            "1 of its member repositories were not read: only the 1 newest pushed are, as advisory_repositories says"
        )
    );
    assert_eq!(
        urls[0].0,
        "https://api.github.com/orgs/o/repos?sort=pushed&direction=desc&per_page=1"
    );
    let (unscoped, urls) = read(
        &scoped(Forge::GitHub),
        Vec::new(),
        (
            Asked::default(),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 1000)),
        ),
    );
    assert_eq!(unscoped.open, []);
    assert!(
        unscoped.unread[0]
            .reason
            .starts_with("its members were never listed")
    );
    assert_eq!(urls, []);
}

#[test]
fn a_next_page_on_a_host_that_only_begins_like_the_api_is_never_asked() {
    let (found, asked) = read(
        &queued(Forge::GitHub),
        vec![
            paged(
                &json!([dependabot(1, None)]),
                "https://api.github.com.evil.example/repos/o/r/code-scanning/alerts?page=2",
            ),
            answer(200, &json!([])),
            answer(200, &json!([])),
        ],
        (
            Asked::default(),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 1000)),
        ),
    );
    assert!(found.more);
    assert_eq!(asked.len(), 3);
    assert!(
        asked.iter().all(|(url, _)| !url.contains("evil.example")),
        "{asked:?}"
    );
}

#[test]
fn the_secret_scanning_listing_is_asked_to_hide_each_secret() {
    let empty = || answer(200, &json!([]));
    let (_, asked) = read(
        &queued(Forge::GitHub),
        vec![empty(), empty(), empty()],
        (
            Asked::default(),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 10)),
        ),
    );
    let hidden: Vec<bool> = asked
        .iter()
        .map(|(url, _)| url.ends_with(HIDE_SECRET))
        .collect();
    let secret: Vec<bool> = asked
        .iter()
        .map(|(url, _)| url.contains("/secret-scanning/"))
        .collect();
    assert_eq!(hidden, secret);
    assert!(secret.contains(&true));
}

#[test]
fn a_gitlab_secret_detection_row_holds_no_file_path_and_none_is_asked() {
    let found_secret = json!({
        "webUrl": "https://gitlab.com/o/r/-/security/vulnerabilities/7",
        "title": "Example Service Key", "state": "DETECTED", "severity": "CRITICAL",
        "reportType": "SECRET_DETECTION", "detectedAt": "2026-11-18T10:00:00Z",
        "location": { "file": "config/keys.yml" },
    });
    let (found, asked) = read(
        &queued(Forge::GitLab),
        vec![answer(
            200,
            &vulnerabilities("project", &[found_secret], None),
        )],
        (
            Asked::default(),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 1000)),
        ),
    );
    let query = asked[0].1["query"].as_str().unwrap().to_owned();
    assert!(!query.contains("SecretDetection"), "{query}");
    assert!(!query.contains("file"), "{query}");
    assert_eq!(found.open[0].tool, "secret-scanning");
    assert!(
        !format!("{found:?}").contains("config/keys.yml"),
        "{found:?}"
    );
}

fn organization(members: &[Listed]) -> Asked<'_> {
    Asked {
        scope: Some(Scope {
            owner: Owner::Organization,
            members,
            narrowed: false,
        }),
        ..Asked::default()
    }
}

#[test]
fn another_repository_s_alerts_neither_use_up_the_most_nor_say_more_were_open() {
    let members = [member("o/pay-api")];
    let empty = || answer(200, &json!([]));
    let (found, asked) = read(
        &scoped(Forge::GitHub),
        vec![
            empty(),
            answer(
                200,
                &json!([
                    dependabot(1, Some("o/other")),
                    dependabot(2, Some("o/other")),
                    dependabot(3, Some("o/other")),
                    dependabot(4, Some("o/pay-api"))
                ]),
            ),
            empty(),
        ],
        (
            organization(&members),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 2)),
        ),
    );
    assert_eq!(asked.len(), 3);
    let read: Vec<&str> = found
        .open
        .iter()
        .map(|advisory| advisory.url.as_str())
        .collect();
    assert_eq!(read, ["https://github.com/o/r/security/dependabot/4"]);
    assert!(!found.more);
    assert_eq!(found.unread, []);
}

#[test]
fn a_held_repository_s_alert_left_out_says_more_and_an_owner_s_unread_pages_are_named() {
    let members = [member("o/pay-api")];
    let empty = || answer(200, &json!([]));
    let (cut, asked) = read(
        &scoped(Forge::GitHub),
        vec![
            empty(),
            answer(
                200,
                &json!([
                    dependabot(1, Some("o/other")),
                    dependabot(2, Some("o/pay-api")),
                    dependabot(3, Some("o/pay-api"))
                ]),
            ),
        ],
        (
            organization(&members),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 1)),
        ),
    );
    assert_eq!(asked.len(), 2);
    assert_eq!(cut.open.len(), 1);
    assert!(cut.more);
    let (short, asked) = read(
        &scoped(Forge::GitHub),
        vec![
            empty(),
            paged(
                &json!([dependabot(1, Some("o/other"))]),
                "https://api.github.com/orgs/o/dependabot/alerts?page=2",
            ),
            empty(),
        ],
        (
            organization(&members),
            &[],
            advising(AdvisoriesFrom::Owner, (20, 100)),
        ),
    );
    assert_eq!(asked.len(), 3);
    assert!(!short.more);
    assert_eq!(short.open, []);
    assert_eq!(short.unread.len(), 1);
    assert_eq!(short.unread[0].repository, "o");
    assert_eq!(
        short.unread[0].reason,
        "GitHub's Dependabot alerts: only its first 1 pages were read, as most_advisories bounds the cost, so later pages may hold more of the repositories the view holds"
    );
}

#[test]
fn a_member_whose_recorded_path_steps_out_of_its_repository_is_never_asked() {
    let members = [member("o/../../user")];
    let organization = Asked {
        scope: Some(Scope {
            owner: Owner::Organization,
            members: &members,
            narrowed: false,
        }),
        ..Asked::default()
    };
    let http = ScriptedHttp::new(Vec::new());
    let queue: &dyn herald_kinds::Queue = &scoped(Forge::GitHub);
    let refused = pollster::block_on(queue.advisories(
        &http,
        (organization, &["o/../../user"]),
        advising(AdvisoriesFrom::Shown, (20, 1000)),
    ));
    assert!(
        matches!(&refused, Err(herald_kinds::Unpolled::Unreadable(why)) if why.contains("\"..\" is empty or a dot segment")),
        "{refused:?}"
    );
    assert_eq!(http.asked().len(), 0);
}
