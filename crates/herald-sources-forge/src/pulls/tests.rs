use std::collections::BTreeMap;

use herald_core::{Attribute, Identity, Validators};
use herald_ports::Response;
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::*;
use crate::SourceOptions;
use crate::options::Pulls;

const PAGE: &str = "https://github.com/o/r";

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

fn choice(pulls: Pulls) -> PullChoice {
    SourceOptions {
        pulls: Some(pulls),
        ..SourceOptions::default()
    }
    .pull_choice()
}

fn every() -> PullChoice {
    choice(Pulls {
        actions: Some(vec![
            PullAction::Opened,
            PullAction::ReadyForReview,
            PullAction::Reopened,
            PullAction::Merged,
            PullAction::Closed,
        ]),
        drafts: Some(true),
        ..Pulls::default()
    })
}

fn pull(number: u64, state: &str, extra: &Value) -> Value {
    let mut pull = json!({
        "number": number, "title": "Bump left-pad", "state": state,
        "html_url": format!("{PAGE}/pull/{number}"),
        "user": { "login": "dependabot[bot]", "type": "Bot" },
        "created_at": "2026-11-18T08:00:00Z", "updated_at": "2026-11-18T10:00:00Z",
        "base": { "ref": "main" }, "head": { "ref": "bump", "sha": "c0ffee" },
        "labels": [{ "name": "deps" }, { "name": "ci" }],
    });
    pull.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    pull
}

fn github_hook(action: &str, pull: &Value) -> Vec<u8> {
    json!({
        "action": action, "pull_request": pull,
        "repository": { "id": 7, "name": "r", "full_name": "o/r", "html_url": PAGE },
    })
    .to_string()
    .into_bytes()
}

#[test]
fn a_pull_request_s_event_carries_what_it_is() {
    let found = delivered(
        Forge::GitHub,
        &github_hook("opened", &pull(4, "open", &json!({}))),
        &every(),
    )
    .unwrap();
    assert_eq!(found.path, "o/r");
    let event = found.event.unwrap();
    assert_eq!(event.kind(), &Kind::PULL_REQUEST);
    assert_eq!(event.identity().as_str(), "github.com/7/pull/4/opened");
    assert_eq!(event.title(), "Pull request #4 opened: Bump left-pad");
    let keys: Vec<&str> = event.attributes().keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "action", "author", "base", "bot", "commit", "draft", "head", "labels", "number",
            "signal"
        ]
    );
    assert_eq!(event.attributes()["bot"], Attribute::Flag(true));
    assert_eq!(
        event.attributes()["commit"],
        Attribute::Text("c0ffee".to_owned())
    );
    assert_eq!(
        event.attributes()["labels"],
        Attribute::Text("deps, ci".to_owned())
    );
    let links: Vec<&str> = event.links().keys().map(String::as_str).collect();
    assert_eq!(links, ["pull"]);
}

#[test]
fn each_github_action_reads_as_what_the_pull_request_did() {
    let merged = json!({ "merged": true, "merged_at": "2026-11-18T11:00:00Z", "merged_by": { "login": "octocat" }, "state": "closed" });
    let closed = json!({ "closed_at": "2026-11-18T12:00:00Z", "state": "closed" });
    let cases = [
        ("opened", json!({}), Some("github.com/7/pull/4/opened")),
        (
            "ready_for_review",
            json!({}),
            Some("github.com/7/pull/4/ready_for_review/1794996000"),
        ),
        (
            "reopened",
            json!({}),
            Some("github.com/7/pull/4/reopened/1794996000"),
        ),
        (
            "closed",
            merged,
            Some("github.com/7/pull/4/merged/1794999600"),
        ),
        (
            "closed",
            closed,
            Some("github.com/7/pull/4/closed/1795003200"),
        ),
        ("synchronize", json!({}), None),
    ];
    for (action, extra, identity) in cases {
        let found = delivered(
            Forge::GitHub,
            &github_hook(action, &pull(4, "open", &extra)),
            &every(),
        )
        .unwrap();
        assert_eq!(
            found.event.as_ref().map(|event| event.identity().as_str()),
            identity,
            "{action}"
        );
    }
    let merged = delivered(
        Forge::Forgejo,
        &github_hook(
            "closed",
            &pull(4, "closed", &json!({ "merged": true, "merged_by": { "login": "octocat" }, "user": { "login": "renovate[bot]" } })),
        ),
        &every(),
    )
    .unwrap()
    .event
    .unwrap();
    assert_eq!(
        merged.attributes()["merged_by"],
        Attribute::Text("octocat".to_owned())
    );
    assert_eq!(merged.attributes()["bot"], Attribute::Flag(true));
    assert_eq!(merged.title(), "Pull request #4 merged: Bump left-pad");
    let person = delivered(
        Forge::GitHub,
        &github_hook(
            "opened",
            &pull(
                4,
                "open",
                &json!({ "user": { "login": "ann", "type": "User" } }),
            ),
        ),
        &every(),
    )
    .unwrap()
    .event
    .unwrap();
    assert_eq!(person.attributes()["bot"], Attribute::Flag(false));
    assert!(delivered(Forge::GitHub, b"{}", &every()).is_err());
}

#[test]
fn a_choice_takes_drafts_authors_and_labels_as_it_says() {
    let draft = pull(4, "open", &json!({ "draft": true }));
    let defaults = SourceOptions::default().pull_choice();
    let announced = |choice: &PullChoice, action: &str, pull: &Value| {
        delivered(Forge::GitHub, &github_hook(action, pull), choice)
            .unwrap()
            .event
            .is_some()
    };
    assert!(!announced(&defaults, "opened", &draft));
    assert!(announced(&defaults, "ready_for_review", &draft));
    assert!(announced(&every(), "opened", &draft));
    assert!(!announced(
        &defaults,
        "reopened",
        &pull(4, "open", &json!({}))
    ));
    let narrow = choice(Pulls {
        authors: vec!["dependabot*".to_owned()],
        labels: vec!["dep*".to_owned()],
        ..Pulls::default()
    });
    assert!(announced(&narrow, "opened", &pull(4, "open", &json!({}))));
    assert!(!announced(
        &narrow,
        "opened",
        &pull(4, "open", &json!({ "labels": [{ "name": "docs" }] }))
    ));
    assert!(!announced(
        &narrow,
        "opened",
        &pull(4, "open", &json!({ "user": { "login": "ann" } }))
    ));
    assert!(!narrow.takes((PullAction::Opened, false), (None, &["deps".to_owned()])));
}

fn gitlab_hook(action: &str, extra: &Value) -> Vec<u8> {
    let mut hook = json!({
        "object_kind": "merge_request",
        "user": { "username": "project_9_bot_abc" },
        "project": { "id": 50, "path_with_namespace": "group/web", "web_url": "https://gitlab.com/group/web", "visibility_level": 20 },
        "object_attributes": {
            "iid": 3, "title": "Add feature", "url": "https://gitlab.com/group/web/-/merge_requests/3",
            "action": action, "draft": false, "source_branch": "feature", "target_branch": "main",
            "created_at": "2026-11-18 08:00:00 UTC", "updated_at": "2026-11-18T10:00:00Z",
        },
        "labels": [{ "title": "backend" }],
    });
    hook.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    hook.to_string().into_bytes()
}

#[test]
fn each_gitlab_action_reads_as_what_the_merge_request_did() {
    let readied = json!({ "changes": { "draft": { "previous": true, "current": false } } });
    let drafted = json!({ "changes": { "draft": { "previous": false, "current": true } } });
    let cases = [
        ("open", json!({}), Some("gitlab.com/50/pull/3/opened")),
        (
            "reopen",
            json!({}),
            Some("gitlab.com/50/pull/3/reopened/1794996000"),
        ),
        (
            "merge",
            json!({}),
            Some("gitlab.com/50/pull/3/merged/1794996000"),
        ),
        (
            "close",
            json!({}),
            Some("gitlab.com/50/pull/3/closed/1794996000"),
        ),
        (
            "update",
            readied,
            Some("gitlab.com/50/pull/3/ready_for_review/1794996000"),
        ),
        ("update", drafted, None),
        (
            "update",
            json!({ "changes": { "draft": { "previous": true, "current": true } } }),
            None,
        ),
        (
            "update",
            json!({ "changes": { "draft": { "previous": false, "current": false } } }),
            None,
        ),
        ("update", json!({}), None),
        ("approved", json!({}), None),
    ];
    for (action, extra, identity) in cases {
        let found = delivered(Forge::GitLab, &gitlab_hook(action, &extra), &every()).unwrap();
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
    assert_eq!(opened.title(), "Merge request !3 opened: Add feature");
    assert_eq!(
        opened.attributes()["author"],
        Attribute::Text("project_9_bot_abc".to_owned())
    );
    assert_eq!(opened.attributes()["bot"], Attribute::Flag(true));
    assert_eq!(
        opened.attributes()["labels"],
        Attribute::Text("backend".to_owned())
    );
    assert_eq!(opened.occurred().to_string(), "2026-11-18T08:00:00Z");
    let merged = delivered(Forge::GitLab, &gitlab_hook("merge", &json!({})), &every())
        .unwrap()
        .event
        .unwrap();
    assert!(!merged.attributes().contains_key("author"));
    assert_eq!(merged.attributes()["bot"], Attribute::Flag(false));
    assert_eq!(
        merged.attributes()["merged_by"],
        Attribute::Text("project_9_bot_abc".to_owned())
    );
    let timeless = json!({ "object_attributes": { "iid": 3, "title": "t", "url": "https://gitlab.com/group/web/-/merge_requests/3", "action": "open" } });
    let refused = delivered(Forge::GitLab, &gitlab_hook("open", &timeless), &every()).unwrap_err();
    assert!(refused.to_string().contains("names no time"), "{refused}");
    let actionless = json!({ "object_attributes": { "iid": 3, "title": "t", "url": "https://gitlab.com/group/web/-/merge_requests/3", "created_at": "2026-11-18T08:00:00Z" } });
    assert_eq!(
        delivered(Forge::GitLab, &gitlab_hook("open", &actionless), &every())
            .unwrap()
            .event,
        None
    );
    let opened_late = json!({ "object_attributes": { "iid": 3, "title": "t", "url": "https://gitlab.com/group/web/-/merge_requests/3", "action": "open", "updated_at": "2026-11-18T10:00:00Z" } });
    let late = delivered(Forge::GitLab, &gitlab_hook("open", &opened_late), &every())
        .unwrap()
        .event
        .unwrap();
    assert_eq!(late.occurred().to_string(), "2026-11-18T10:00:00Z");
}

fn asking(forge: Forge) -> Origin {
    let options = SourceOptions {
        repository: Some("o/r".to_owned()),
        ..SourceOptions::default()
    };
    Origin::new(forge, &options, "o/r")
}

fn polled(
    origin: &Origin,
    http: &ScriptedHttp,
    cursor: Option<&Cursor>,
) -> Result<(bool, Found), Unpolled> {
    let choice = every();
    let asking = Asking {
        origin,
        token: None,
        choice: &choice,
    };
    let http = Erased(http as &dyn DynHttp);
    let asked = pollster::block_on(asking.asked(&http, cursor))?;
    let found = asking.read(&asked, &repository())?;
    Ok((asked.changed(), found))
}

#[test]
fn a_poll_reads_each_forge_s_newest_pull_requests_under_their_validators() {
    let merged = json!({ "merged_at": "2026-11-18T11:00:00Z" });
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!([
            pull(1, "open", &json!({})),
            pull(2, "closed", &merged),
            pull(3, "closed", &json!({ "closed_at": "2026-11-18T12:00:00Z" }))
        ]),
        Some("\"p\""),
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
    let (changed, found) = polled(&asking(Forge::GitHub), &http, Some(&cursor)).unwrap();
    assert!(changed);
    assert_eq!(
        http.asked()[0],
        (
            "https://api.github.com/repos/o/r/pulls?state=all&sort=updated&direction=desc&per_page=30".to_owned(),
            Some("\"old\"".to_owned())
        )
    );
    let identities: Vec<&Identity> = found.events.iter().map(Event::identity).collect();
    assert_eq!(
        identities,
        [
            &Identity::new("github.com/7/pull/1/opened"),
            &Identity::new("github.com/7/pull/2/merged/1794999600"),
            &Identity::new("github.com/7/pull/3/closed/1795003200"),
        ]
    );
    assert_eq!(found.listings[KEPT].etag.as_deref(), Some("\"p\""));
    let quiet = ScriptedHttp::new(vec![answer(304, &Value::Null, None)]);
    let (changed, found) = polled(&asking(Forge::GitHub), &quiet, Some(&cursor)).unwrap();
    assert!(!changed);
    assert_eq!(found.listings[KEPT].etag.as_deref(), Some("\"old\""));
    let merge = |iid: u64, state: &str| {
        json!({
            "iid": iid, "title": "t", "web_url": format!("https://gitlab.com/o/r/-/merge_requests/{iid}"),
            "state": state, "author": { "username": "renovate-bot" }, "merged_by": { "username": "ann" },
            "merged_at": "2026-11-18T11:00:00Z", "closed_at": "2026-11-18T12:00:00Z",
            "created_at": "2026-11-18T08:00:00Z", "updated_at": null, "labels": ["deps"],
            "source_branch": "renovate/x", "target_branch": "main", "sha": format!("5ha{iid}"),
        })
    };
    let gitlab = ScriptedHttp::new(vec![answer(
        200,
        &json!([
            merge(1, "opened"),
            merge(2, "merged"),
            merge(3, "closed"),
            merge(4, "locked")
        ]),
        None,
    )]);
    let (_, found) = polled(&asking(Forge::GitLab), &gitlab, None).unwrap();
    assert!(
        gitlab.asked()[0]
            .0
            .ends_with("/merge_requests?state=all&order_by=updated_at&sort=desc&per_page=30")
    );
    let read: Vec<(&str, &Attribute)> = found
        .events
        .iter()
        .map(|event| (event.identity().as_str(), &event.attributes()["bot"]))
        .collect();
    assert_eq!(
        read,
        [
            ("github.com/7/pull/1/opened", &Attribute::Flag(true)),
            (
                "github.com/7/pull/2/merged/1794999600",
                &Attribute::Flag(true)
            ),
            (
                "github.com/7/pull/3/closed/1795003200",
                &Attribute::Flag(true)
            ),
        ]
    );
    assert_eq!(
        found.events[0].attribute(COMMIT),
        Some(&Attribute::Text("5ha1".to_owned()))
    );
    let forgejo = ScriptedHttp::new(vec![answer(200, &json!([]), None)]);
    polled(&asking(Forge::Forgejo), &forgejo, None).unwrap();
    assert!(
        forgejo.asked()[0]
            .0
            .ends_with("/pulls?state=all&sort=recentupdate&limit=30")
    );
}

#[test]
fn a_listing_that_cannot_be_read_is_named_in_the_poll_s_reason() {
    for (forge, what) in [
        (Forge::GitHub, "GitHub's pull requests"),
        (Forge::GitLab, "GitLab's merge requests"),
        (Forge::Forgejo, "Forgejo's pull requests"),
    ] {
        let refused = ScriptedHttp::new(vec![answer(500, &json!({}), None)]);
        match polled(&asking(forge), &refused, None) {
            Err(Unpolled::Unreadable(reason)) => assert!(reason.contains(what), "{reason}"),
            other => panic!("{forge:?}: {other:?}"),
        }
    }
    let unparsed = ScriptedHttp::new(vec![answer(200, &json!({}), None)]);
    assert!(matches!(
        polled(&asking(Forge::GitLab), &unparsed, None),
        Err(Unpolled::Unreadable(_))
    ));
    let unlinked = ScriptedHttp::new(vec![answer(
        200,
        &json!([pull(1, "open", &json!({ "html_url": "nowhere" }))]),
        None,
    )]);
    let origin = asking(Forge::GitHub);
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
            assert!(reason.contains("GitHub's pull requests"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
    let unreachable = ScriptedHttp::new(Vec::new());
    assert!(matches!(
        polled(&origin, &unreachable, None),
        Err(Unpolled::Unreachable(_))
    ));
    assert!(hooked(Forge::GitHub, "pull_request"));
    assert!(hooked(Forge::Forgejo, "pull_request"));
    assert!(!hooked(Forge::GitHub, "Merge Request Hook"));
    assert!(hooked(Forge::GitLab, "Merge Request Hook"));
    assert!(!hooked(Forge::GitLab, "pull_request"));
}

#[test]
fn a_delivery_that_cannot_be_read_names_its_forge_s_event_and_a_person_is_no_bot() {
    for (forge, what) in [
        (Forge::GitHub, "GitHub's pull request event"),
        (Forge::GitLab, "GitLab's merge request event"),
        (Forge::Forgejo, "Forgejo's pull request event"),
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
fn a_merge_request_s_event_names_its_head_commit_where_the_hook_does() {
    let committed = json!({ "object_attributes": {
        "iid": 3, "title": "Add feature", "url": "https://gitlab.com/group/web/-/merge_requests/3",
        "action": "open", "created_at": "2026-11-18T08:00:00Z", "last_commit": { "id": "f00d" },
    } });
    let commit = |extra: &Value| {
        delivered(Forge::GitLab, &gitlab_hook("open", extra), &every())
            .unwrap()
            .event
            .unwrap()
            .attribute(COMMIT)
            .cloned()
    };
    assert_eq!(commit(&committed), Some(Attribute::Text("f00d".to_owned())));
    assert_eq!(commit(&json!({})), None);
}
