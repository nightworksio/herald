use herald_core::{Checks, Review, Visibility};
use herald_ports::Secret;
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::*;
use crate::queue::MOST_PULLS;
use crate::{Origin, SourceOptions, Token};

fn answer(status: u16, etag: Option<&str>, body: &Value) -> Response {
    Response {
        status,
        headers: etag
            .map(|etag| ("ETag".to_owned(), etag.to_owned()))
            .into_iter()
            .collect(),
        body: body.to_string().into_bytes(),
    }
}

fn unchanged() -> Response {
    Response {
        status: 304,
        headers: Vec::new(),
        body: Vec::new(),
    }
}

fn tagged(etag: &str) -> Validators {
    Validators {
        etag: Some(etag.to_owned()),
        last_modified: None,
    }
}

fn rows(forge: Forge, numbers: std::ops::Range<u64>) -> Value {
    let rows: Vec<Value> = numbers
        .map(|number| match forge {
            Forge::GitLab => json!({
                "iid": number, "title": "t", "web_url": "https://gitlab.com/o/r/-/merge_requests/1",
                "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
                "project_id": 9
            }),
            Forge::GitHub | Forge::Forgejo => json!({
                "number": number, "title": "t", "html_url": "https://github.com/o/r/pull/1",
                "created_at": "2026-11-18T10:00:00Z", "updated_at": "2026-11-18T11:00:00Z",
                "head": { "sha": "abc" }, "base": { "repo": { "id": 42 } }
            }),
        })
        .collect();
    Value::from(rows)
}

struct Asked {
    answer: Result<Relisted, Unpolled>,
    asked: Vec<(String, Option<String>)>,
}

fn relisted(forge: Forge, tokened: bool, kept: &[Validators], answers: Vec<Response>) -> Asked {
    let origin = Origin::new(
        forge,
        &SourceOptions {
            repository: Some("o/r".to_owned()),
            ..SourceOptions::default()
        },
        "o/r",
    );
    let token = Token::Named(Secret::new("TOKEN", "t"));
    let opened = Opened {
        origin: &origin,
        token: tokened.then_some(&token),
        project: "r",
        graphql: "https://api.github.com/graphql",
        visibility: Some(Visibility::Public),
        most_open: 2 * PAGE,
        most_pulls: MOST_PULLS,
        stated: false,
        files: false,
    };
    let http = ScriptedHttp::new(answers);
    let answer = pollster::block_on(opened.relisted(&Erased(&http as &dyn DynHttp), kept));
    Asked {
        answer,
        asked: http.asked(),
    }
}

fn listed(asked: Asked) -> (Vec<u64>, Option<String>, Vec<Validators>) {
    let Ok(Relisted::Listed {
        pulls,
        repository,
        validators,
        ..
    }) = asked.answer
    else {
        panic!("not listed: {:?}", asked.answer);
    };
    (
        pulls.iter().map(|pull| pull.number).collect(),
        repository,
        validators,
    )
}

#[test]
fn a_first_reconcile_lists_every_page_and_keeps_each_page_s_validators_and_the_repository() {
    let asked = relisted(
        Forge::GitHub,
        true,
        &[],
        vec![
            answer(200, Some("\"a\""), &rows(Forge::GitHub, 0..100)),
            answer(200, Some("\"b\""), &rows(Forge::GitHub, 100..101)),
        ],
    );
    assert_eq!(
        asked.asked,
        [
            (
                "https://api.github.com/repos/o/r/pulls?state=open&sort=updated&direction=desc&per_page=100&page=1".to_owned(),
                None
            ),
            (
                "https://api.github.com/repos/o/r/pulls?state=open&sort=updated&direction=desc&per_page=100&page=2".to_owned(),
                None
            ),
        ]
    );
    let (numbers, repository, validators) = listed(asked);
    assert_eq!(numbers.len(), 101);
    assert_eq!(repository.as_deref(), Some("42"));
    assert_eq!(validators, [tagged("\"a\""), tagged("\"b\"")]);
}

#[test]
fn a_listing_whose_every_page_is_unchanged_reads_nothing() {
    let asked = relisted(
        Forge::GitHub,
        false,
        &[tagged("\"a\""), tagged("\"b\"")],
        vec![unchanged(), unchanged()],
    );
    assert_eq!(asked.answer, Ok(Relisted::Unchanged));
    let tags: Vec<Option<String>> = asked.asked.into_iter().map(|(_, tag)| tag).collect();
    assert_eq!(tags, [Some("\"a\"".to_owned()), Some("\"b\"".to_owned())]);
}

#[test]
fn a_changed_page_lists_the_whole_again_reading_the_changed_page_once() {
    let asked = relisted(
        Forge::GitHub,
        false,
        &[tagged("\"a\""), tagged("\"b\"")],
        vec![
            unchanged(),
            answer(200, Some("\"c\""), &rows(Forge::GitHub, 100..102)),
            answer(200, Some("\"a\""), &rows(Forge::GitHub, 0..100)),
        ],
    );
    let pages: Vec<(&str, Option<&str>)> = asked
        .asked
        .iter()
        .map(|(url, tag)| (&url[url.len() - 6..], tag.as_deref()))
        .collect();
    assert_eq!(
        pages,
        [
            ("page=1", Some("\"a\"")),
            ("page=2", Some("\"b\"")),
            ("page=1", None)
        ]
    );
    let (numbers, _, validators) = listed(asked);
    assert_eq!(numbers.len(), 102);
    assert_eq!(validators, [tagged("\"a\""), tagged("\"c\"")]);
}

#[test]
fn gitlab_names_its_project_and_forgejo_ends_at_an_empty_page() {
    let (numbers, repository, _) = listed(relisted(
        Forge::GitLab,
        true,
        &[],
        vec![answer(200, None, &rows(Forge::GitLab, 1..3))],
    ));
    assert_eq!((numbers, repository.as_deref()), (vec![1, 2], Some("9")));
    let asked = relisted(
        Forge::Forgejo,
        false,
        &[],
        vec![
            answer(200, None, &rows(Forge::Forgejo, 1..2)),
            answer(200, None, &json!([])),
        ],
    );
    assert_eq!(asked.asked.len(), 2);
    assert_eq!(listed(asked).0, [1]);
}

#[test]
fn a_page_the_forge_refuses_or_that_cannot_be_read_stops_the_listing() {
    let refused = relisted(
        Forge::GitHub,
        false,
        &[],
        vec![answer(500, None, &json!({}))],
    );
    assert!(
        matches!(&refused.answer, Err(Unpolled::Unreadable(why)) if why.contains(listing::GITHUB)),
        "{:?}",
        refused.answer
    );
    let garbled = relisted(
        Forge::GitHub,
        false,
        &[],
        vec![answer(200, None, &json!({}))],
    );
    assert!(matches!(garbled.answer, Err(Unpolled::Unreadable(_))));
    let unreached = relisted(Forge::GitHub, false, &[tagged("\"a\"")], Vec::new());
    assert!(matches!(unreached.answer, Err(Unpolled::Unreachable(_))));
}

fn listed_pull(number: u64) -> Pull {
    Pull {
        project: "r".to_owned(),
        repository: "o/r".to_owned(),
        visibility: Visibility::Private,
        number,
        title: "t".to_owned(),
        url: "https://github.com/o/r/pull/1".to_owned(),
        author: None,
        bot: false,
        draft: false,
        labels: Vec::new(),
        opened: jiff::Timestamp::UNIX_EPOCH,
        updated: jiff::Timestamp::UNIX_EPOCH,
        review: Review::Requested,
        reviewers: Vec::new(),
        checks: Checks::Absent,
        mergeable: herald_core::Mergeable::Unknown,
        queued: false,
        forge: None,
        head: Some("abc".to_owned()),
        unread: false,
        base: None,
        files: None,
        areas: Vec::new(),
        flips: Vec::new(),
    }
}

fn completed(
    forge: Forge,
    tokened: bool,
    answers: Vec<Response>,
) -> (Result<Vec<Pull>, Unpolled>, ScriptedHttp) {
    let origin = Origin::new(
        forge,
        &SourceOptions {
            repository: Some("o/r".to_owned()),
            ..SourceOptions::default()
        },
        "o/r",
    );
    let token = Token::Named(Secret::new("TOKEN", "t"));
    let opened = Opened {
        origin: &origin,
        token: tokened.then_some(&token),
        project: "r",
        graphql: "https://api.github.com/graphql",
        visibility: None,
        most_open: PAGE,
        most_pulls: MOST_PULLS,
        stated: false,
        files: false,
    };
    let http = ScriptedHttp::new(answers);
    let read = pollster::block_on(opened.completed(
        &Erased(&http as &dyn DynHttp),
        vec![listed_pull(7), listed_pull(8)],
    ));
    (read, http)
}

#[test]
fn gitlab_s_listing_is_read_in_full_already() {
    let (read, http) = completed(Forge::GitLab, true, Vec::new());
    assert_eq!(read.unwrap(), [listed_pull(7), listed_pull(8)]);
    assert_eq!(http.asked(), []);
}

#[test]
fn a_rest_read_asks_each_pull_request_s_reviews_and_checks() {
    let reviews = json!([{ "user": { "login": "octo" }, "state": "APPROVED",
        "author_association": "MEMBER" }]);
    let checks = json!({ "check_runs": [{ "name": "test", "status": "completed",
        "conclusion": "failure" }] });
    let (read, http) = completed(
        Forge::GitHub,
        false,
        vec![
            answer(200, None, &reviews),
            answer(200, None, &checks),
            answer(200, None, &json!([])),
            answer(200, None, &json!({ "check_runs": [] })),
        ],
    );
    let read = read.unwrap();
    assert_eq!(http.asked().len(), 4);
    assert_eq!(read[0].review, Review::Approved);
    assert_eq!(read[0].checks, Checks::Failed(vec!["test".to_owned()]));
    assert_eq!(read[1].review, Review::Requested);
}

#[test]
fn github_s_graphql_reads_a_page_of_them_a_request_leaving_out_one_it_holds_none_for() {
    let node = json!({
        "number": 7, "title": "read", "url": "https://github.com/o/r/pull/7",
        "isDraft": false, "createdAt": "2026-11-18T10:00:00Z",
        "updatedAt": "2026-11-18T11:00:00Z", "reviewDecision": "APPROVED"
    });
    let body = json!({ "data": { "repository": {
        "visibility": "PUBLIC", "p0": node, "p1": null
    } } });
    let (read, http) = completed(Forge::GitHub, true, vec![answer(200, None, &body)]);
    let read = read.unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!(
        (read[0].title.as_str(), read[0].review, read[0].visibility),
        ("read", Review::Approved, Visibility::Public)
    );
    let query = http.bodies()[0]["query"].as_str().unwrap().to_owned();
    assert!(query.contains("p0: pullRequest(number: 7)"));
    assert!(query.contains("p1: pullRequest(number: 8)"));
    assert_eq!(
        http.bodies()[0]["variables"],
        json!({ "owner": "o", "name": "r" })
    );
    let (unnamed, _) = completed(
        Forge::GitHub,
        true,
        vec![answer(
            200,
            None,
            &json!({ "data": { "repository": null } }),
        )],
    );
    assert!(matches!(unnamed, Err(Unpolled::Unreadable(_))));
    let (garbled, _) = completed(Forge::GitHub, true, vec![answer(200, None, &json!([]))]);
    assert!(matches!(garbled, Err(Unpolled::Unreadable(_))));
}
