use std::collections::BTreeMap;

use herald_core::{Attribute, Identity, Validators};
use herald_ports::Response;
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::*;
use crate::SourceOptions;
use crate::options::Issues;

const PAGE: &str = "https://github.com/o/r";

fn delivered(forge: Forge, payload: &[u8], choice: &IssueChoice) -> Result<Delivered, SourceError> {
    let name = if forge == Forge::GitLab {
        gitlab::HOOK
    } else {
        github::HOOK
    };
    super::delivered(forge, (name, payload), choice)
}

fn repository() -> Repository {
    Repository {
        id: 7,
        name: "r".to_owned(),
        page: Some(PAGE.to_owned()),
    }
}

fn answer(status: u16, body: &Value, etag: Option<&str>) -> Response {
    Response {
        status,
        headers: etag
            .map(|etag| vec![("etag".to_owned(), etag.to_owned())])
            .unwrap_or_default(),
        body: body.to_string().into_bytes(),
    }
}

fn every() -> IssueChoice {
    SourceOptions::default().issue_choice()
}

fn issue(number: u64, state: &str, extra: &Value) -> Value {
    let mut issue = json!({
        "number": number, "title": "Crash on start", "state": state,
        "html_url": format!("{PAGE}/issues/{number}"),
        "user": { "login": "ann", "type": "User" },
        "created_at": "2026-11-18T08:00:00Z", "updated_at": "2026-11-18T10:00:00Z",
        "closed_at": "2026-11-18T11:00:00Z", "state_reason": "not_planned",
        "labels": [{ "name": "bug" }],
    });
    issue
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    issue
}

fn github_hook(action: &str, issue: &Value) -> Vec<u8> {
    json!({
        "action": action, "issue": issue,
        "repository": { "id": 7, "name": "r", "full_name": "o/r", "html_url": PAGE },
    })
    .to_string()
    .into_bytes()
}

#[test]
fn each_github_action_reads_as_what_the_issue_did() {
    let cases = [
        ("opened", Some("github.com/7/issue/4/opened")),
        ("closed", Some("github.com/7/issue/4/closed/1794999600")),
        ("reopened", Some("github.com/7/issue/4/reopened/1794996000")),
        ("labeled", None),
    ];
    for (action, identity) in cases {
        let found = delivered(
            Forge::GitHub,
            &github_hook(action, &issue(4, "open", &json!({}))),
            &every(),
        )
        .unwrap();
        assert_eq!(found.path, "o/r");
        assert_eq!(
            found.event.as_ref().map(|event| event.identity().as_str()),
            identity,
            "{action}"
        );
    }
    let closed = delivered(
        Forge::GitHub,
        &github_hook("closed", &issue(4, "closed", &json!({}))),
        &every(),
    )
    .unwrap()
    .event
    .unwrap();
    assert_eq!(closed.kind(), &Kind::ISSUE);
    assert_eq!(closed.title(), "Issue #4 closed: Crash on start");
    let keys: Vec<&str> = closed.attributes().keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "action",
            "author",
            "bot",
            "labels",
            "number",
            "signal",
            "state_reason"
        ]
    );
    assert_eq!(
        closed.attributes()["state_reason"],
        Attribute::Text("not_planned".to_owned())
    );
    let links: Vec<&str> = closed.links().keys().map(String::as_str).collect();
    assert_eq!(links, ["issue"]);
    let opened = delivered(
        Forge::Forgejo,
        &github_hook(
            "opened",
            &issue(
                4,
                "open",
                &json!({ "user": { "login": "renovate[bot]" }, "labels": [] }),
            ),
        ),
        &every(),
    )
    .unwrap()
    .event
    .unwrap();
    assert!(!opened.attributes().contains_key("state_reason"));
    assert!(!opened.attributes().contains_key("labels"));
    assert_eq!(opened.attributes()["bot"], Attribute::Flag(true));
    let unclosed = issue(
        4,
        "closed",
        &json!({ "closed_at": null, "updated_at": null }),
    );
    let at = delivered(Forge::GitHub, &github_hook("closed", &unclosed), &every())
        .unwrap()
        .event
        .unwrap();
    assert_eq!(at.occurred().to_string(), "2026-11-18T08:00:00Z");
    assert!(delivered(Forge::GitHub, b"{}", &every()).is_err());
}

#[test]
fn a_choice_takes_actions_authors_and_labels_as_it_says() {
    let narrow = SourceOptions {
        issues: Some(Issues {
            actions: Some(vec![IssueAction::Opened]),
            authors: vec!["a*".to_owned()],
            labels: vec!["bu*".to_owned()],
            confidential: None,
        }),
        ..SourceOptions::default()
    }
    .issue_choice();
    let announced = |action: &str, extra: &Value| {
        delivered(
            Forge::GitHub,
            &github_hook(action, &issue(4, "open", extra)),
            &narrow,
        )
        .unwrap()
        .event
        .is_some()
    };
    assert!(announced("opened", &json!({})));
    assert!(!announced("closed", &json!({})));
    assert!(!announced("opened", &json!({ "user": { "login": "bob" } })));
    assert!(!announced(
        "opened",
        &json!({ "labels": [{ "name": "docs" }] })
    ));
    assert!(!narrow.takes((IssueAction::Opened, false), (None, &["bug".to_owned()])));
    let names: Vec<&str> = [
        IssueAction::Opened,
        IssueAction::Closed,
        IssueAction::Reopened,
    ]
    .into_iter()
    .map(IssueAction::name)
    .collect();
    assert_eq!(names, ["opened", "closed", "reopened"]);
}

fn gitlab_hook(action: &str, extra: &Value) -> Vec<u8> {
    let mut hook = json!({
        "object_kind": "issue",
        "user": { "username": "triage-bot" },
        "project": { "id": 50, "path_with_namespace": "group/web", "web_url": "https://gitlab.com/group/web", "visibility_level": 0 },
        "object_attributes": {
            "iid": 3, "title": "Broken", "url": "https://gitlab.com/group/web/-/issues/3", "action": action,
            "created_at": "2026-11-18 08:00:00 UTC", "updated_at": "2026-11-18T10:00:00Z",
        },
        "labels": [{ "title": "bug" }],
    });
    hook.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    hook.to_string().into_bytes()
}

#[test]
fn each_gitlab_action_reads_as_what_the_issue_did() {
    let cases = [
        ("open", Some("gitlab.com/50/issue/3/opened")),
        ("close", Some("gitlab.com/50/issue/3/closed/1794996000")),
        ("reopen", Some("gitlab.com/50/issue/3/reopened/1794996000")),
        ("update", None),
    ];
    for (action, identity) in cases {
        let found = delivered(Forge::GitLab, &gitlab_hook(action, &json!({})), &every()).unwrap();
        assert_eq!(found.path, "group/web");
        assert_eq!(
            found.event.as_ref().map(|event| event.identity().as_str()),
            identity,
            "{action}"
        );
    }
    let opened = delivered(Forge::GitLab, &gitlab_hook("open", &json!({})), &every())
        .unwrap()
        .event
        .unwrap();
    assert_eq!(
        opened.attributes()["author"],
        Attribute::Text("triage-bot".to_owned())
    );
    assert_eq!(opened.attributes()["bot"], Attribute::Flag(true));
    assert_eq!(opened.occurred().to_string(), "2026-11-18T08:00:00Z");
    let closed = delivered(Forge::GitLab, &gitlab_hook("close", &json!({})), &every())
        .unwrap()
        .event
        .unwrap();
    assert!(!closed.attributes().contains_key("author"));
    assert_eq!(closed.attributes()["bot"], Attribute::Flag(false));
    let timeless = json!({ "object_attributes": { "iid": 3, "title": "t", "url": "https://gitlab.com/group/web/-/issues/3", "action": "open" } });
    let refused = delivered(Forge::GitLab, &gitlab_hook("open", &timeless), &every()).unwrap_err();
    assert!(refused.to_string().contains("names no time"), "{refused}");
    let late = json!({ "object_attributes": { "iid": 3, "title": "t", "url": "https://gitlab.com/group/web/-/issues/3", "action": "open", "updated_at": "2026-11-18T10:00:00Z" } });
    let opened = delivered(Forge::GitLab, &gitlab_hook("open", &late), &every())
        .unwrap()
        .event
        .unwrap();
    assert_eq!(opened.occurred().to_string(), "2026-11-18T10:00:00Z");
    let actionless = json!({ "object_attributes": { "iid": 3, "title": "t", "url": "https://gitlab.com/group/web/-/issues/3", "created_at": "2026-11-18T08:00:00Z" } });
    assert_eq!(
        delivered(Forge::GitLab, &gitlab_hook("open", &actionless), &every())
            .unwrap()
            .event,
        None
    );
    assert!(hooked(Forge::GitLab, "Issue Hook"));
    assert!(!hooked(Forge::GitLab, "issues"));
    assert!(hooked(Forge::GitHub, "issues"));
    assert!(hooked(Forge::Forgejo, "issues"));
    assert!(!hooked(Forge::GitHub, "Issue Hook"));
}

fn polled(
    forge: Forge,
    http: &ScriptedHttp,
    cursor: Option<&Cursor>,
) -> Result<(bool, Found), Unpolled> {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        ..SourceOptions::default()
    };
    let origin = Origin::new(forge, &options, "o/r");
    let choice = every();
    let asking = Asking {
        origin: &origin,
        token: None,
        choice: &choice,
    };
    let http = Erased(http as &dyn DynHttp);
    let asked = pollster::block_on(asking.asked(&http, cursor))?;
    let found = asking.read(&asked, &repository())?;
    Ok((asked.changed(), found))
}

#[test]
fn a_poll_reads_each_forge_s_newest_issues_under_their_validators() {
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!([
            issue(1, "open", &json!({})),
            issue(2, "closed", &json!({})),
            issue(3, "open", &json!({ "pull_request": {} }))
        ]),
        Some("\"i\""),
    )]);
    let cursor = Cursor {
        listings: BTreeMap::from([(
            KEPT.to_owned(),
            Validators {
                etag: Some("\"old\"".to_owned()),
                last_modified: None,
            },
        )]),
        ..Cursor::default()
    };
    let (changed, found) = polled(Forge::GitHub, &http, Some(&cursor)).unwrap();
    assert!(changed);
    assert_eq!(
        http.asked()[0],
        (
            "https://api.github.com/repos/o/r/issues?state=all&sort=updated&direction=desc&per_page=30".to_owned(),
            Some("\"old\"".to_owned())
        )
    );
    let identities: Vec<&Identity> = found.events.iter().map(Event::identity).collect();
    assert_eq!(
        identities,
        [
            &Identity::new("github.com/7/issue/1/opened"),
            &Identity::new("github.com/7/issue/2/closed/1794999600")
        ]
    );
    assert_eq!(found.listings[KEPT].etag.as_deref(), Some("\"i\""));
    let quiet = ScriptedHttp::new(vec![answer(304, &Value::Null, None)]);
    let (changed, found) = polled(Forge::GitHub, &quiet, Some(&cursor)).unwrap();
    assert!(!changed);
    assert_eq!(found.listings[KEPT].etag.as_deref(), Some("\"old\""));
    let listed = |iid: u64, state: &str| {
        json!({
            "iid": iid, "title": "t", "web_url": format!("https://gitlab.com/o/r/-/issues/{iid}"), "state": state,
            "author": { "username": "ann" }, "closed_at": null, "created_at": "2026-11-18T08:00:00Z",
            "updated_at": "2026-11-18T10:00:00Z", "labels": ["bug"],
        })
    };
    let gitlab = ScriptedHttp::new(vec![answer(
        200,
        &json!([listed(1, "opened"), listed(2, "closed")]),
        None,
    )]);
    let (_, found) = polled(Forge::GitLab, &gitlab, None).unwrap();
    assert!(gitlab.asked()[0].0.ends_with(
        "/issues?state=all&order_by=updated_at&sort=desc&per_page=30&confidential=false"
    ));
    let identities: Vec<&str> = found
        .events
        .iter()
        .map(|event| event.identity().as_str())
        .collect();
    assert_eq!(
        identities,
        [
            "github.com/7/issue/1/opened",
            "github.com/7/issue/2/closed/1794996000"
        ]
    );
    let forgejo = ScriptedHttp::new(vec![answer(200, &json!([]), None)]);
    polled(Forge::Forgejo, &forgejo, None).unwrap();
    assert!(
        forgejo.asked()[0]
            .0
            .ends_with("/issues?state=all&type=issues&sort=recentupdate&limit=30")
    );
}

#[test]
fn a_listing_that_cannot_be_read_is_named_in_the_poll_s_reason() {
    for (forge, what) in [
        (Forge::GitHub, "GitHub's issues"),
        (Forge::GitLab, "GitLab's issues"),
        (Forge::Forgejo, "Forgejo's issues"),
    ] {
        let refused = ScriptedHttp::new(vec![answer(500, &json!({}), None)]);
        match polled(forge, &refused, None) {
            Err(Unpolled::Unreadable(reason)) => assert!(reason.contains(what), "{reason}"),
            other => panic!("{forge:?}: {other:?}"),
        }
    }
    let unparsed = ScriptedHttp::new(vec![answer(200, &json!({}), None)]);
    assert!(matches!(
        polled(Forge::GitLab, &unparsed, None),
        Err(Unpolled::Unreadable(_))
    ));
    let unlinked = ScriptedHttp::new(vec![answer(
        200,
        &json!([issue(1, "open", &json!({ "html_url": "nowhere" }))]),
        None,
    )]);
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        ..SourceOptions::default()
    };
    let origin = Origin::new(Forge::GitHub, &options, "o/r");
    let choice = every();
    let asking = Asking {
        origin: &origin,
        token: None,
        choice: &choice,
    };
    let http = Erased(&unlinked as &dyn DynHttp);
    let asked = pollster::block_on(asking.asked(&http, None)).unwrap();
    let lost = Repository {
        page: None,
        ..repository()
    };
    assert!(matches!(
        asking.read(&asked, &lost),
        Err(Unpolled::Unreadable(_))
    ));
    match asking.read(&asked, &repository()) {
        Err(Unpolled::Unreadable(reason)) => {
            assert!(reason.contains("GitHub's issues"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
    let unreachable = ScriptedHttp::new(Vec::new());
    assert!(matches!(
        polled(Forge::GitHub, &unreachable, None),
        Err(Unpolled::Unreachable(_))
    ));
}

fn confidential() -> IssueChoice {
    SourceOptions {
        issues: Some(Issues {
            confidential: Some(true),
            ..Issues::default()
        }),
        ..SourceOptions::default()
    }
    .issue_choice()
}

#[test]
fn a_confidential_gitlab_issue_is_announced_only_where_asked_and_then_as_private() {
    let secret = json!({ "object_attributes": {
        "iid": 3, "title": "Leak", "url": "https://gitlab.com/group/web/-/issues/3",
        "action": "open", "confidential": true, "created_at": "2026-11-18T08:00:00Z",
    } });
    let hooked = gitlab_hook("open", &secret);
    assert_eq!(
        delivered(Forge::GitLab, &hooked, &every()).unwrap().event,
        None
    );
    let shown = delivered(Forge::GitLab, &hooked, &confidential())
        .unwrap()
        .event
        .unwrap();
    assert_eq!(shown.visibility(), Some(Visibility::Private));
    let open = delivered(
        Forge::GitLab,
        &gitlab_hook("open", &json!({})),
        &confidential(),
    )
    .unwrap()
    .event
    .unwrap();
    assert_eq!(open.visibility(), None);
    assert!(super::hooked(Forge::GitLab, "Confidential Issue Hook"));
    let unmarked = json!({ "object_attributes": {
        "iid": 3, "title": "Leak", "url": "https://gitlab.com/group/web/-/issues/3",
        "action": "open", "created_at": "2026-11-18T08:00:00Z",
    } });
    for payload in [
        gitlab_hook("open", &unmarked),
        gitlab_hook(
            "open",
            &json!({ "object_attributes": {
        "iid": 3, "title": "Leak", "url": "https://gitlab.com/group/web/-/issues/3",
        "action": "open", "confidential": false, "created_at": "2026-11-18T08:00:00Z",
    } }),
        ),
    ] {
        let through = |choice: &IssueChoice| {
            super::delivered(Forge::GitLab, ("Confidential Issue Hook", &payload), choice)
                .unwrap()
                .event
        };
        assert_eq!(through(&every()), None);
        assert_eq!(
            through(&confidential()).unwrap().visibility(),
            Some(Visibility::Private)
        );
        assert_eq!(
            delivered(Forge::GitLab, &payload, &every())
                .unwrap()
                .event
                .unwrap()
                .visibility(),
            None
        );
    }
    let listed = |iid: u64, confidential: bool| {
        json!({
            "iid": iid, "title": "t", "web_url": format!("https://gitlab.com/o/r/-/issues/{iid}"), "state": "opened",
            "author": { "username": "ann" }, "closed_at": null, "created_at": "2026-11-18T08:00:00Z",
            "updated_at": null, "labels": [], "confidential": confidential,
        })
    };
    let body = json!([listed(1, false), listed(2, true)]);
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        ..SourceOptions::default()
    };
    let origin = Origin::new(Forge::GitLab, &options, "o/r");
    for (choice, path, visibilities) in [
        (
            every(),
            "/issues?state=all&order_by=updated_at&sort=desc&per_page=30&confidential=false",
            vec![None],
        ),
        (
            confidential(),
            "/issues?state=all&order_by=updated_at&sort=desc&per_page=30",
            vec![None, Some(Visibility::Private)],
        ),
    ] {
        let http = ScriptedHttp::new(vec![answer(200, &body, None)]);
        let asking = Asking {
            origin: &origin,
            token: None,
            choice: &choice,
        };
        let erased = Erased(&http as &dyn DynHttp);
        let asked = pollster::block_on(asking.asked(&erased, None)).unwrap();
        assert!(http.asked()[0].0.ends_with(path), "{}", http.asked()[0].0);
        let found = asking.read(&asked, &repository()).unwrap();
        let seen: Vec<Option<Visibility>> = found.events.iter().map(Event::visibility).collect();
        assert_eq!(seen, visibilities);
    }
}

#[test]
fn a_delivery_that_cannot_be_read_names_its_forge_s_event_and_a_person_is_no_bot() {
    for (forge, what) in [
        (Forge::GitHub, "GitHub's issue event"),
        (Forge::GitLab, "GitLab's issue event"),
        (Forge::Forgejo, "Forgejo's issue event"),
    ] {
        match delivered(forge, b"{}", &every()) {
            Err(SourceError::Unreadable { what: named, .. }) => assert_eq!(named, what),
            other => panic!("{forge:?}: {other:?}"),
        }
    }
    let person = json!({ "user": { "username": "ann" } });
    let opened = delivered(Forge::GitLab, &gitlab_hook("open", &person), &every())
        .unwrap()
        .event
        .unwrap();
    assert_eq!(opened.attributes()["bot"], Attribute::Flag(false));
}

#[test]
fn a_github_bot_account_is_a_bot_whatever_its_login() {
    let app = json!({ "user": { "login": "deployer", "type": "Bot" } });
    let opened = delivered(
        Forge::GitHub,
        &github_hook("opened", &issue(4, "open", &app)),
        &every(),
    )
    .unwrap()
    .event
    .unwrap();
    assert_eq!(opened.attributes()["bot"], Attribute::Flag(true));
}
