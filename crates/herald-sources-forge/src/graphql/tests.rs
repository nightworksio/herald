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
        ..Cursor::default()
    }
}

fn together(
    http: &Scripted,
    members: &[(&Listed, Option<&Cursor>)],
) -> Vec<Result<Option<Polled>, Unpolled>> {
    let signalled: Vec<(&Listed, Option<&Cursor>, Signals)> = members
        .iter()
        .map(|(member, cursor)| (*member, *cursor, Signals::default()))
        .collect();
    together_signalled(http, &signalled)
}

fn together_signalled(
    http: &Scripted,
    members: &[(&Listed, Option<&Cursor>, Signals)],
) -> Vec<Result<Option<Polled>, Unpolled>> {
    watching(http, members, 0)
}

fn watching(
    http: &Scripted,
    members: &[(&Listed, Option<&Cursor>, Signals)],
    watched: usize,
) -> Vec<Result<Option<Polled>, Unpolled>> {
    let token = Token::Named(Secret::new("TOKEN", "t"));
    let asking = Asking {
        forge: Forge::GitHub,
        endpoint: "https://api.github.com/graphql",
        token: Some(&token),
        page: 30,
        parse: 1 << 20,
        watched,
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
        is_prerelease: None,
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

fn tagged(id: u64, releases: &[Value], tags: &[Value]) -> Value {
    json!({
        "databaseId": id,
        "url": format!("https://github.com/lemonfiber/m{id}"),
        "releases": { "nodes": releases },
        "refs": { "nodes": tags }
    })
}

fn signals(taken: &[Signal]) -> Signals {
    Signals {
        taken: taken.iter().copied().collect(),
        prerelease: herald_core::PrereleaseRule::Either,
    }
}

#[test]
fn each_member_is_asked_for_the_releases_and_tags_its_signals_take() {
    let (cli, web) = (member("cli"), member("web"));
    let http = Scripted::new(vec![data(&[tagged(7, &[], &[]), tagged(8, &[], &[])])]);
    let answered = together_signalled(
        &http,
        &[
            (&cli, Some(&seen(&[])), signals(&[Signal::Tag])),
            (
                &web,
                Some(&seen(&[])),
                signals(&[Signal::Release, Signal::Tag]),
            ),
        ],
    );
    assert_eq!(answered, [Ok(None), Ok(None)]);
    let query = http.bodies()[0]["query"].as_str().unwrap().to_owned();
    let m0 = &query[query.find("m0:").unwrap()..query.find("m1:").unwrap()];
    assert!(!m0.contains("releases("), "{m0}");
    assert!(m0.contains("refs(refPrefix: \"refs/tags/\", first: $page, orderBy: {field: TAG_COMMIT_DATE, direction: DESC}) { nodes { name } }"), "{m0}");
    let m1 = &query[query.find("m1:").unwrap()..];
    assert!(m1.contains("releases(") && m1.contains("refs("), "{m1}");
    assert!(m1.contains("databaseId url"), "{m1}");
}

#[test]
fn a_tag_not_seen_is_read_in_full_with_its_date_and_a_tag_of_a_release_is_the_release_s() {
    let cli = member("cli");
    let cursor = seen(&["github.com/7/v1"]);
    let annotated = json!({
        "name": "v3-rc.1",
        "target": {
            "__typename": "Tag",
            "message": "Candidate",
            "tagger": {"date": "2026-11-03T12:00:00Z"},
            "target": {"committedDate": "2026-11-02T12:00:00Z"}
        }
    });
    let lightweight = json!({
        "name": "v2",
        "target": {"__typename": "Commit", "committedDate": "2026-10-30T12:00:00Z"}
    });
    let released = json!({"name": "v1", "target": {"__typename": "Commit", "committedDate": "2026-10-01T12:00:00Z"}});
    let http = Scripted::new(vec![
        data(&[tagged(
            7,
            &[release("v1", false)],
            &[
                json!({"name": "v3-rc.1"}),
                json!({"name": "v2"}),
                json!({"name": "v1"}),
            ],
        )]),
        data(&[tagged(
            7,
            &[release("v1", false)],
            &[annotated, lightweight, released],
        )]),
    ]);
    let answered = together_signalled(
        &http,
        &[(
            &cli,
            Some(&cursor),
            signals(&[Signal::Release, Signal::Tag]),
        )],
    );
    let Ok(Some(polled)) = &answered[0] else {
        panic!("{answered:?}");
    };
    let found: Vec<(&str, Option<Signal>, bool)> = polled
        .events
        .iter()
        .map(|event| {
            (
                event.identity().as_str(),
                event.signal(),
                event.prerelease(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            ("github.com/7/v3-rc.1", Some(Signal::Tag), true),
            ("github.com/7/v1", Some(Signal::Release), false),
            ("github.com/7/v2", Some(Signal::Tag), false),
        ]
    );
    assert_eq!(polled.events[0].body(), "Candidate");
    assert_eq!(
        polled.events[0].occurred().to_string(),
        "2026-11-03T12:00:00Z"
    );
    assert_eq!(
        polled.events[0].link("release"),
        Some("https://github.com/lemonfiber/m7/releases/tag/v3-rc.1")
    );
    assert_eq!(polled.events[2].body(), "");
    assert_eq!(
        polled.events[2].occurred().to_string(),
        "2026-10-30T12:00:00Z"
    );
    assert!(http.bodies()[1]["query"].as_str().unwrap().contains(DATED));
}

#[test]
fn a_member_whose_tags_its_cursor_has_seen_is_not_asked_in_full() {
    let cli = member("cli");
    let http = Scripted::new(vec![data(&[tagged(7, &[], &[json!({"name": "v1"})])])]);
    let answered = together_signalled(
        &http,
        &[(
            &cli,
            Some(&seen(&["github.com/7/v1"])),
            signals(&[Signal::Tag]),
        )],
    );
    assert_eq!(answered, [Ok(None)]);
    assert_eq!(http.bodies().len(), 1);
}

#[test]
fn a_tag_of_an_annotated_tag_or_an_undated_one_takes_what_it_points_at() {
    let nested = Ref {
        name: "v1".to_owned(),
        target: Some(Target {
            message: Some(" ".to_owned()),
            tagger: None,
            pointed: Some(Box::new(Target {
                message: None,
                tagger: None,
                pointed: None,
                committed_date: Some("2026-11-01T00:00:00Z".parse().unwrap()),
            })),
            committed_date: None,
        }),
    }
    .read();
    assert_eq!(nested.message, None);
    assert_eq!(nested.date, Some("2026-11-01T00:00:00Z".parse().unwrap()));
    let bare = Ref {
        name: "v0".to_owned(),
        target: None,
    }
    .read();
    assert_eq!((bare.message, bare.date), (None, None));
}

#[test]
fn tags_of_a_repository_github_names_no_page_of_cannot_be_announced() {
    let cli = member("cli");
    let pageless = json!({
        "databaseId": 7,
        "refs": { "nodes": [{"name": "v1", "target": {"committedDate": "2026-11-01T00:00:00Z"}}] }
    });
    let http = Scripted::new(vec![
        data(std::slice::from_ref(&pageless)),
        data(&[pageless]),
    ]);
    let answered = together_signalled(&http, &[(&cli, None, signals(&[Signal::Tag]))]);
    assert_eq!(
        answered,
        [Err(Unpolled::Unreadable(
            "GitHub's GraphQL answer names no web page of lemonfiber/cli, which its tags are announced by".to_owned()
        ))]
    );
    let empty = json!({"databaseId": 7, "refs": {"nodes": []}});
    let http = Scripted::new(vec![data(std::slice::from_ref(&empty)), data(&[empty])]);
    let answered = together_signalled(&http, &[(&cli, None, signals(&[Signal::Tag]))]);
    assert_eq!(answered, [Ok(Some(Polled::default()))]);
}

#[test]
fn a_release_of_a_version_its_cursor_saw_only_as_a_tag_is_asked_in_full() {
    let cli = member("cli");
    let cursor = Cursor {
        tagged: vec![Identity::new("github.com/7/v1")],
        ..seen(&["github.com/7/v1"])
    };
    let http = Scripted::new(vec![
        data(&[tagged(7, &[release("v1", false)], &[])]),
        data(&[tagged(7, &[release("v1", false)], &[])]),
    ]);
    let answered = together_signalled(&http, &[(&cli, Some(&cursor), signals(&[Signal::Release]))]);
    assert!(matches!(&answered[0], Ok(Some(polled)) if polled.events.len() == 1));
    let seen_as_release = Scripted::new(vec![data(&[tagged(7, &[release("v1", false)], &[])])]);
    let answered = together_signalled(
        &seen_as_release,
        &[(
            &cli,
            Some(&seen(&["github.com/7/v1"])),
            signals(&[Signal::Release]),
        )],
    );
    assert_eq!(answered, [Ok(None)]);
}

#[test]
fn a_member_watched_for_edits_is_asked_its_notes_and_read_in_full_once_one_changed() {
    let api = member("api");
    let noted = |notes: &str| {
        let mut edited = release("v2", false);
        edited["description"] = json!(notes);
        data(&[tagged(9, &[edited, release("v1", false)], &[])])
    };
    let http = Scripted::new(vec![noted("## Added"), noted("## Added")]);
    let first = watching(&http, &[(&api, None, Signals::default())], 1);
    let Ok(Some(polled)) = &first[0] else {
        panic!("a member with no cursor is read in full: {first:?}");
    };
    assert_eq!(polled.watched, 1);
    let cursor = Cursor::default().advanced(polled, 500);
    assert_eq!(cursor.notes.len(), 1);
    let http = Scripted::new(vec![noted("## Added")]);
    let quiet = watching(&http, &[(&api, Some(&cursor), Signals::default())], 1);
    assert!(matches!(quiet[..], [Ok(None)]), "{quiet:?}");
    let asked = http.bodies()[0]["query"].as_str().unwrap().to_owned();
    assert!(asked.contains("description"), "{asked}");
    let http = Scripted::new(vec![noted("## Fixed"), noted("## Fixed")]);
    let changed = watching(&http, &[(&api, Some(&cursor), Signals::default())], 1);
    let Ok(Some(polled)) = &changed[0] else {
        panic!("an edited release is read in full: {changed:?}");
    };
    let found = herald_core::fresh(Some(&cursor), polled.events.clone(), 0);
    assert_eq!(found.len(), 1);
    assert!(found[0].edited());
    let unwatched = Scripted::new(vec![noted("## Fixed")]);
    let light = watching(&unwatched, &[(&api, Some(&cursor), Signals::default())], 0);
    assert!(matches!(light[..], [Ok(None)]));
    let asked = unwatched.bodies()[0]["query"].as_str().unwrap().to_owned();
    assert!(!asked.contains("description"), "{asked}");
}

fn projects(nodes: &[Value]) -> Response {
    Response {
        status: 200,
        headers: Vec::new(),
        body: json!({ "data": { "projects": { "nodes": nodes } } })
            .to_string()
            .into_bytes(),
    }
}

fn gitlab_release(tag: &str, upcoming: bool) -> Value {
    json!({
        "tagName": tag,
        "name": format!("cli {tag}"),
        "description": "## Added",
        "releasedAt": "2026-11-01T12:00:00Z",
        "upcomingRelease": upcoming,
        "links": { "selfUrl": format!("https://gitlab.com/lemonfiber/cli/-/releases/{tag}") }
    })
}

fn project(id: u64, releases: &[Value]) -> Value {
    json!({
        "id": format!("gid://gitlab/Project/{id}"),
        "webUrl": "https://gitlab.com/lemonfiber/cli",
        "releases": { "nodes": releases }
    })
}

fn on_gitlab(
    http: &Scripted,
    members: &[(&Listed, Option<&Cursor>)],
) -> Vec<Result<Option<Polled>, Unpolled>> {
    let asking = Asking {
        forge: Forge::GitLab,
        endpoint: "https://gitlab.com/api/graphql",
        token: None,
        page: 30,
        parse: 1 << 20,
        watched: 0,
    };
    let signalled: Vec<(&Listed, Option<&Cursor>, Signals)> = members
        .iter()
        .map(|(member, cursor)| (*member, *cursor, Signals::default()))
        .collect();
    pollster::block_on(polled_together(
        &Erased(http as &dyn DynHttp),
        &asking,
        &signalled,
    ))
}

#[test]
fn a_gitlab_owner_s_members_are_asked_by_project_id_and_announce_releases_as_rest_names_them() {
    let cli = Listed {
        id: "7".to_owned(),
        ..member("cli")
    };
    let web = Listed {
        id: "8".to_owned(),
        ..member("web")
    };
    let gone = Listed {
        id: "9".to_owned(),
        ..member("gone")
    };
    let http = Scripted::new(vec![
        projects(&[
            project(7, &[gitlab_release("v1", false)]),
            project(8, &[gitlab_release("v1", false)]),
            json!({ "id": "gid://gitlab/Group/9", "webUrl": null, "releases": null }),
        ]),
        projects(&[project(
            7,
            &[gitlab_release("v2", true), gitlab_release("v1", false)],
        )]),
    ]);
    let answered = on_gitlab(
        &http,
        &[
            (&cli, None),
            (&web, Some(&seen(&["gitlab.com/8/v1"]))),
            (&gone, None),
        ],
    );
    let Ok(Some(polled)) = &answered[0] else {
        panic!("an unseen member is read in full: {answered:?}");
    };
    assert_eq!(polled.events.len(), 1);
    assert_eq!(polled.events[0].identity().as_str(), "gitlab.com/7/v1");
    assert!(!polled.events[0].prerelease());
    assert_eq!(answered[1], Ok(None));
    assert_eq!(
        answered[2],
        Err(Unpolled::Unreadable(
            "GitLab's GraphQL answer names no repository for member 2; the token may not reach it"
                .to_owned()
        ))
    );
    let bodies = http.bodies();
    assert_eq!(
        bodies[0]["variables"],
        json!({ "ids": ["gid://gitlab/Project/7", "gid://gitlab/Project/8", "gid://gitlab/Project/9"], "page": 30 })
    );
    let query = bodies[0]["query"].as_str().unwrap();
    assert!(query.contains("projects(ids: $ids, first: 100)"), "{query}");
    assert!(
        query.contains("nodes { tagName upcomingRelease links { selfUrl } }"),
        "{query}"
    );
    assert!(bodies[1]["query"].as_str().unwrap().contains("releasedAt"));
    assert_eq!(
        bodies[1]["variables"]["ids"],
        json!(["gid://gitlab/Project/7"])
    );
    assert_eq!(
        http.requests()[0].url.redacted(),
        "https://gitlab.com/api/graphql"
    );
    let failed = Scripted::new(vec![Response {
        status: 200,
        headers: Vec::new(),
        body: br#"{"data":null,"errors":[{"message":"complexity"}]}"#.to_vec(),
    }]);
    assert_eq!(
        on_gitlab(&failed, &[(&cli, None)]),
        [Err(Unpolled::Unreadable(
            r#"GitLab's GraphQL answer holds no data: [{"message":"complexity"}]"#.to_owned()
        ))]
    );
}
