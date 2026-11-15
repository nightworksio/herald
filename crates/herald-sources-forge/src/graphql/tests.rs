use std::collections::BTreeMap;

use herald_core::Visibility;
use herald_ports::Secret;

use super::*;
use crate::scripted::Scripted;

fn data(members: &[Value]) -> Response {
    let data: serde_json::Map<String, Value> = members
        .iter()
        .enumerate()
        .map(|(index, member)| (format!("m{index}"), member.clone()))
        .collect();
    Response {
        status: 200,
        headers: Vec::new(),
        body: json!({ "data": data }).to_string().into_bytes(),
    }
}

fn release(tag: &str, draft: bool) -> Value {
    json!({
        "tagName": tag,
        "url": format!("https://github.com/lemonfiber/cli/releases/tag/{tag}"),
        "isDraft": draft,
        "name": format!("cli {tag}"),
        "description": "## Added",
        "isPrerelease": false,
        "publishedAt": "2026-11-01T12:00:00Z"
    })
}

fn repository(id: u64, releases: &[Value]) -> Value {
    json!({ "databaseId": id, "releases": { "nodes": releases } })
}

fn member(name: &str) -> Listed {
    Listed {
        id: "1".to_owned(),
        name: name.to_owned(),
        path: format!("lemonfiber/{name}"),
        topics: Vec::new(),
        properties: BTreeMap::new(),
        visibility: Visibility::Public,
        archived: false,
        fork: false,
        template: false,
        language: None,
    }
}

fn seen(identities: &[&str]) -> Cursor {
    Cursor {
        seen: identities.iter().map(|seen| Identity::new(*seen)).collect(),
        etag: None,
        last_modified: None,
        project: None,
    }
}

fn together(
    http: &Scripted,
    members: &[(&Listed, Option<&Cursor>)],
) -> Vec<Result<Option<Polled>, Unpolled>> {
    let token = Token::Named(Secret::new("TOKEN", "t"));
    let asking = Asking {
        endpoint: "https://api.github.com/graphql",
        token: Some(&token),
        page: 30,
        parse: 1 << 20,
    };
    pollster::block_on(polled_together(
        &Erased(http as &dyn DynHttp),
        &asking,
        members,
    ))
}

#[test]
fn one_request_asks_each_member_by_variables_with_the_source_s_token() {
    let cli = member("cli");
    let web = Listed {
        path: "lemonfiber/sub/web".to_owned(),
        ..member("web")
    };
    let http = Scripted::new(vec![data(&[repository(7, &[]), repository(8, &[])])]);
    let answered = together(&http, &[(&cli, Some(&seen(&[]))), (&web, Some(&seen(&[])))]);
    assert_eq!(answered, [Ok(None), Ok(None)]);
    let asked = http.requests();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].method, Method::Post);
    assert_eq!(asked[0].url.redacted(), "https://api.github.com/graphql");
    let headers: Vec<(String, String)> = asked[0]
        .headers
        .iter()
        .map(|(name, value)| (name.clone(), value.redacted()))
        .collect();
    assert!(headers.contains(&("Content-Type".to_owned(), "application/json".to_owned())));
    assert!(headers.iter().any(|(name, _)| name == "Authorization"));
    let body = &http.bodies()[0];
    assert_eq!(
        body["variables"],
        json!({ "o0": "lemonfiber", "n0": "cli", "o1": "lemonfiber/sub", "n1": "web", "page": 30 })
    );
    let query = body["query"].as_str().unwrap();
    assert!(query.starts_with("query($o0: String!, $n0: String!, $o1: String!, $n1: String!, $page: Int!) { m0: repository(owner: $o0, name: $n0)"));
    assert!(query.contains("nodes { tagName url isDraft }"));
    assert!(!query.contains("lemonfiber"));
}

#[test]
fn only_members_holding_a_release_their_cursor_has_not_seen_are_asked_in_full() {
    let (cli, web, api) = (member("cli"), member("web"), member("api"));
    let cursor = seen(&["github.com/7/v1"]);
    let http = Scripted::new(vec![
        data(&[
            repository(7, &[release("v1", false), release("v2-draft", true)]),
            repository(8, &[release("v1", false)]),
            repository(9, &[release("v1", false)]),
        ]),
        data(&[repository(8, &[release("v1", false), release("v0", true)])]),
    ]);
    let answered = together(
        &http,
        &[
            (&cli, Some(&cursor)),
            (&web, None),
            (&api, Some(&seen(&["github.com/9/v1"]))),
        ],
    );
    assert_eq!(answered[0], Ok(None));
    assert_eq!(answered[2], Ok(None));
    let Ok(Some(polled)) = &answered[1] else {
        panic!("a member never polled is read in full: {answered:?}");
    };
    assert_eq!(polled.events.len(), 1);
    assert_eq!(polled.events[0].identity().as_str(), "github.com/8/v1");
    assert_eq!(polled.events[0].project(), "web");
    assert_eq!(polled.etag, None);
    let bodies = http.bodies();
    assert_eq!(bodies.len(), 2);
    assert_eq!(
        bodies[1]["variables"],
        json!({ "o0": "lemonfiber", "n0": "web", "page": 30 })
    );
    assert!(bodies[1]["query"].as_str().unwrap().contains(FULL));
}

#[test]
fn a_member_github_names_no_repository_for_fails_alone_and_a_failed_request_fails_them_all() {
    let (cli, web) = (member("cli"), member("web"));
    let http = Scripted::new(vec![data(&[Value::Null, repository(8, &[])])]);
    let answered = together(&http, &[(&cli, None), (&web, Some(&seen(&[])))]);
    assert_eq!(answered[1], Ok(None));
    assert!(
        matches!(&answered[0], Err(Unpolled::Unreadable(reason)) if reason.contains("names no repository for member 0"))
    );
    let silent = Scripted::new(Vec::new());
    assert!(
        together(&silent, &[(&cli, None), (&web, None)])
            .iter()
            .all(|answered| matches!(answered, Err(Unpolled::Unreachable(_))))
    );
    let refused = Scripted::new(vec![Response {
        status: 502,
        headers: Vec::new(),
        body: Vec::new(),
    }]);
    assert!(
        together(&refused, &[(&cli, None)])
            .iter()
            .all(|answered| matches!(answered, Err(Unpolled::Unreadable(_))))
    );
    let errored = Scripted::new(vec![Response {
        status: 200,
        headers: Vec::new(),
        body: br#"{"errors":[{"message":"rate limited"}]}"#.to_vec(),
    }]);
    let answered = together(&errored, &[(&cli, None), (&web, None)]);
    assert!(answered.iter().all(|answered| matches!(
        answered,
        Err(Unpolled::Unreadable(reason)) if reason.contains("holds no data") && reason.contains("rate limited")
    )));
    let second_fails = Scripted::new(vec![data(&[repository(7, &[release("v1", false)])])]);
    assert!(matches!(
        together(&second_fails, &[(&cli, None)])[0],
        Err(Unpolled::Unreachable(_))
    ));
}

#[test]
fn a_release_github_answers_without_a_publication_time_is_unreadable() {
    let cli = member("cli");
    let mut unpublished = release("v1", false);
    unpublished["publishedAt"] = Value::Null;
    let http = Scripted::new(vec![
        data(&[repository(7, &[unpublished.clone()])]),
        data(&[repository(7, &[unpublished])]),
    ]);
    assert!(matches!(
        together(&http, &[(&cli, None)])[0],
        Err(Unpolled::Unreadable(_))
    ));
}

#[test]
fn a_release_whose_page_names_no_host_is_never_seen() {
    let node = Node {
        tag_name: "v1".to_owned(),
        url: "not a url".to_owned(),
        is_draft: false,
        name: None,
        description: None,
        is_prerelease: false,
        published_at: None,
    };
    assert_eq!(identity(7, &node), None);
}

#[test]
fn a_member_whose_cursor_has_not_seen_its_newest_release_is_asked_in_full() {
    let api = member("api");
    let http = Scripted::new(vec![
        data(&[repository(9, &[release("v2", false), release("v1", false)])]),
        data(&[repository(9, &[release("v2", false), release("v1", false)])]),
    ]);
    let answered = together(&http, &[(&api, Some(&seen(&["github.com/9/v1"])))]);
    let Ok(Some(polled)) = &answered[0] else {
        panic!("an unseen release is read in full: {answered:?}");
    };
    assert_eq!(polled.events.len(), 2);
    assert_eq!(http.bodies().len(), 2);
}
