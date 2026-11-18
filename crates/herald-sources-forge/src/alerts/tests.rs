use std::collections::BTreeMap;

use herald_core::{Attribute, Cursor, Identity, Validators};
use herald_ports::{Method, Response};
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

use super::*;
use crate::SourceOptions;

mod github_alerts;
mod gitlab_alerts;

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

fn alert(tool: Tool, action: AlertAction) -> ForgeAlert {
    ForgeAlert {
        tool,
        number: 3,
        action,
        severity: Some(Severity::High),
        rule: Some("js/xss".to_owned()),
        summary: None,
        package: None,
        ecosystem: None,
        path: None,
        resolution: None,
        url: format!("{PAGE}/security/code-scanning/3"),
        occurred: "2026-11-18T09:00:00Z".parse().unwrap(),
    }
}

#[test]
fn an_alert_s_event_carries_what_the_alert_is_and_nothing_else() {
    let full = ForgeAlert {
        summary: Some("Cross-site scripting".to_owned()),
        package: Some("left-pad".to_owned()),
        ecosystem: Some("npm".to_owned()),
        path: Some("src/a.js".to_owned()),
        resolution: Some("revoked".to_owned()),
        ..alert(Tool::CodeScanning, AlertAction::Created)
    };
    let event = full.event("alerts", &repository()).unwrap();
    assert_eq!(event.kind(), &Kind::ALERT);
    assert_eq!(
        event.identity().as_str(),
        "github.com/7/alert/code-scanning/3/created"
    );
    assert_eq!(
        event.title(),
        "Code scanning alert #3 opened: Cross-site scripting"
    );
    assert_eq!(event.body(), "");
    let keys: Vec<&str> = event.attributes().keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "action",
            "ecosystem",
            "number",
            "package",
            "path",
            "resolution",
            "rule",
            "severity",
            "signal",
            "tool"
        ]
    );
    assert_eq!(event.attributes()["number"], Attribute::Number(3.into()));
    assert_eq!(
        event.attributes()["severity"],
        Attribute::Text("high".to_owned())
    );
    let links: Vec<&str> = event.links().keys().map(String::as_str).collect();
    assert_eq!(links, ["alert"]);
    let bare = ForgeAlert {
        severity: None,
        rule: None,
        ..alert(Tool::SecretScanning, AlertAction::Fixed)
    }
    .event("alerts", &repository())
    .unwrap();
    let keys: Vec<&str> = bare.attributes().keys().map(String::as_str).collect();
    assert_eq!(keys, ["action", "number", "signal", "tool"]);
    assert_eq!(bare.title(), "Secret scanning alert #3 fixed");
    assert_eq!(
        bare.identity().as_str(),
        "github.com/7/alert/secret-scanning/3/fixed/1794992400"
    );
}

#[test]
fn each_tool_and_action_titles_its_alert() {
    let titled = |tool, action| {
        alert(tool, action)
            .event("alerts", &repository())
            .unwrap()
            .title()
            .to_owned()
    };
    let tools: Vec<String> = Tool::ALL
        .into_iter()
        .map(|tool| titled(tool, AlertAction::Created))
        .collect();
    assert_eq!(
        tools,
        [
            "Code scanning alert #3 opened: js/xss",
            "Dependency scanning alert #3 opened: js/xss",
            "Secret scanning alert #3 opened: js/xss",
            "Container scanning alert #3 opened: js/xss",
            "DAST alert #3 opened: js/xss",
            "Fuzzing alert #3 opened: js/xss",
            "Security alert #3 opened: js/xss",
        ]
    );
    let actions: Vec<String> = [
        AlertAction::Reopened,
        AlertAction::Confirmed,
        AlertAction::Fixed,
        AlertAction::Dismissed,
    ]
    .into_iter()
    .map(|action| titled(Tool::Other, action))
    .collect();
    assert_eq!(
        actions,
        [
            "Security alert #3 reopened: js/xss",
            "Security alert #3 confirmed: js/xss",
            "Security alert #3 fixed: js/xss",
            "Security alert #3 dismissed: js/xss",
        ]
    );
}

#[test]
fn an_alert_whose_page_names_no_host_makes_no_event() {
    let unpaged = Repository {
        page: None,
        ..repository()
    };
    let lost = || ForgeAlert {
        url: "not a page".to_owned(),
        ..alert(Tool::Other, AlertAction::Created)
    };
    assert!(lost().event("alerts", &unpaged).is_err());
    let unlinked = lost().event("alerts", &repository()).unwrap_err();
    assert!(
        matches!(unlinked, SourceError::Invalid { what: "alerts", .. }),
        "{unlinked:?}"
    );
}

#[test]
fn a_delivery_is_of_an_alert_where_its_forge_names_one() {
    assert_eq!(
        Delivery::named(Forge::GitHub, "dependabot_alert"),
        Some(Delivery::GitHub(Scanner::Dependabot))
    );
    assert_eq!(Delivery::named(Forge::GitHub, "release"), None);
    assert_eq!(
        Delivery::named(Forge::GitLab, "Vulnerability Hook"),
        Some(Delivery::GitLab)
    );
    assert_eq!(Delivery::named(Forge::GitLab, "Pipeline Hook"), None);
    assert_eq!(Delivery::named(Forge::Forgejo, "dependabot_alert"), None);
    assert_eq!(
        Delivery::GitHub(Scanner::SecretScanning).what(),
        "GitHub's secret scanning alert event"
    );
    assert_eq!(Delivery::GitLab.what(), "GitLab's vulnerability event");
    let headers = [("X-GitHub-Event", "code_scanning_alert")];
    assert_eq!(
        Forge::GitHub.alert_delivery(&headers),
        Some(Delivery::GitHub(Scanner::CodeScanning))
    );
    assert_eq!(Forge::GitLab.alert_delivery(&headers), None);
}

fn asking(forge: Forge, path: &str) -> (Origin, AlertChoice) {
    let options = SourceOptions {
        repository: Some(path.to_owned()),
        ..SourceOptions::default()
    };
    (
        Origin::new(forge, &options, path),
        options.alert_choice(forge),
    )
}

fn polled(
    (origin, choice): (&Origin, &AlertChoice),
    http: &ScriptedHttp,
    cursor: Option<&Cursor>,
) -> Result<(bool, Found), Unpolled> {
    let asking = Asking {
        origin,
        token: None,
        choice,
    };
    let http = Erased(http as &dyn DynHttp);
    let asked = pollster::block_on(asking.asked(&http, cursor))?;
    let found = asking.read(&asked, &repository())?;
    Ok((asked.changed(), found))
}

fn listed_alert(number: u64, state: &str) -> Value {
    json!({
        "number": number,
        "state": state,
        "html_url": format!("{PAGE}/security/dependabot/{number}"),
        "created_at": "2026-11-18T08:00:00Z",
        "fixed_at": "2026-11-18T09:00:00Z",
        "security_advisory": { "ghsa_id": "GHSA-1", "severity": "critical", "summary": "Bad" },
    })
}

#[test]
fn a_github_poll_asks_each_chosen_tool_s_listing_under_its_own_validators() {
    let (origin, _) = asking(Forge::GitHub, "o/r");
    let choice = SourceOptions {
        alerts: Some(crate::Alerts {
            actions: Some(vec![AlertAction::Created, AlertAction::Fixed]),
            ..crate::Alerts::default()
        }),
        ..SourceOptions::default()
    }
    .alert_choice(Forge::GitHub);
    let http = ScriptedHttp::new(vec![
        answer(200, &json!([listed_alert(1, "open")]), Some("\"c\"")),
        answer(304, &Value::Null, None),
        answer(
            200,
            &json!([listed_alert(2, "fixed"), listed_alert(3, "weird")]),
            Some("\"s\""),
        ),
    ]);
    let cursor = Cursor {
        listings: BTreeMap::from([(
            "alert/dependabot".to_owned(),
            Validators {
                etag: Some("\"d\"".to_owned()),
                last_modified: None,
            },
        )]),
        ..Cursor::default()
    };
    let (changed, found) = polled((&origin, &choice), &http, Some(&cursor)).unwrap();
    assert!(changed);
    let asked: Vec<String> = http.asked().into_iter().map(|(url, _)| url).collect();
    assert_eq!(
        asked,
        [
            "https://api.github.com/repos/o/r/code-scanning/alerts?sort=updated&direction=desc&per_page=30",
            "https://api.github.com/repos/o/r/dependabot/alerts?sort=updated&direction=desc&per_page=30",
            "https://api.github.com/repos/o/r/secret-scanning/alerts?sort=updated&direction=desc&per_page=30",
        ]
    );
    assert_eq!(http.asked()[1].1.as_deref(), Some("\"d\""));
    let identities: Vec<&Identity> = found.events.iter().map(Event::identity).collect();
    assert_eq!(
        identities,
        [
            &Identity::new("github.com/7/alert/code-scanning/1/created"),
            &Identity::new("github.com/7/alert/secret-scanning/2/fixed/1794992400"),
        ]
    );
    let kept: Vec<(&str, Option<&str>)> = found
        .listings
        .iter()
        .map(|(name, kept)| (name.as_str(), kept.etag.as_deref()))
        .collect();
    assert_eq!(
        kept,
        [
            ("alert/code-scanning", Some("\"c\"")),
            ("alert/dependabot", Some("\"d\"")),
            ("alert/secret-scanning", Some("\"s\"")),
        ]
    );
    assert_eq!(found.validators, None);
    let quiet = ScriptedHttp::new(vec![
        answer(304, &Value::Null, None),
        answer(304, &Value::Null, None),
        answer(304, &Value::Null, None),
    ]);
    let (changed, found) = polled((&origin, &choice), &quiet, None).unwrap();
    assert!(!changed);
    assert_eq!(found.events, []);
}

#[test]
fn a_poll_announces_only_the_alerts_its_choice_takes_and_says_why_it_cannot_read() {
    let (origin, _) = asking(Forge::GitHub, "o/r");
    let options = SourceOptions {
        alerts: Some(crate::Alerts {
            tools: Some(vec![Tool::DependencyScanning]),
            severities: vec![Severity::Low],
            ..crate::Alerts::default()
        }),
        ..SourceOptions::default()
    };
    let choice = options.alert_choice(Forge::GitHub);
    let http = ScriptedHttp::new(vec![answer(200, &json!([listed_alert(1, "open")]), None)]);
    let (_, found) = polled((&origin, &choice), &http, None).unwrap();
    assert_eq!(found.events, []);
    assert_eq!(http.asked().len(), 1);
    let refused = ScriptedHttp::new(vec![answer(403, &json!({}), None)]);
    match polled((&origin, &choice), &refused, None) {
        Err(Unpolled::Unreadable(reason)) => {
            assert!(reason.contains("GitHub's Dependabot alerts"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
    let unlinked = ScriptedHttp::new(vec![answer(
        200,
        &json!([{ "number": 1, "state": "open", "html_url": "nowhere", "created_at": "2026-11-18T08:00:00Z" }]),
        None,
    )]);
    let dependencies = SourceOptions {
        alerts: Some(crate::Alerts {
            tools: Some(vec![Tool::DependencyScanning]),
            ..crate::Alerts::default()
        }),
        ..SourceOptions::default()
    }
    .alert_choice(Forge::GitHub);
    let unpaged = Asking {
        origin: &origin,
        token: None,
        choice: &dependencies,
    };
    let http = Erased(&unlinked as &dyn DynHttp);
    let asked = pollster::block_on(unpaged.asked(&http, None)).unwrap();
    let lost = Repository {
        page: None,
        ..repository()
    };
    assert!(matches!(
        unpaged.read(&asked, &lost),
        Err(Unpolled::Unreadable(_))
    ));
    let unreachable = ScriptedHttp::new(Vec::new());
    assert!(matches!(
        polled((&origin, &choice), &unreachable, None),
        Err(Unpolled::Unreachable(_))
    ));
}

#[test]
fn a_gitlab_poll_asks_graphql_for_the_newest_detected_and_forgejo_is_asked_nothing() {
    let (origin, choice) = asking(Forge::GitLab, "group/web");
    let http = ScriptedHttp::new(vec![answer(
        200,
        &json!({ "data": { "project": { "vulnerabilities": { "nodes": [
            gitlab_alerts::node("DETECTED", 4),
            gitlab_alerts::node("PENDING_VERIFICATION", 5),
        ] } } } }),
        Some("\"ignored\""),
    )]);
    let (changed, found) = polled((&origin, &choice), &http, None).unwrap();
    assert!(changed);
    let request = &http.requests()[0];
    assert_eq!(request.method, Method::Post);
    let asked: Vec<String> = http.asked().into_iter().map(|(url, _)| url).collect();
    assert_eq!(asked, ["https://gitlab.com/api/graphql"]);
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(
        body["variables"],
        json!({ "path": "group/web", "page": 30 })
    );
    assert!(
        body["query"]
            .as_str()
            .unwrap()
            .contains("vulnerabilities(first: $page, sort: detected_desc)")
    );
    let identities: Vec<&Identity> = found.events.iter().map(Event::identity).collect();
    assert_eq!(
        identities,
        [&Identity::new(
            "github.com/7/alert/dependency-scanning/4/created"
        )]
    );
    assert_eq!(found.listings, BTreeMap::new());
    let (origin, choice) = asking(Forge::Forgejo, "o/r");
    let none = ScriptedHttp::new(Vec::new());
    let (changed, found) = polled((&origin, &choice), &none, None).unwrap();
    assert!(!changed);
    assert_eq!(found.events, []);
    assert_eq!(none.requests().len(), 0);
}

#[test]
fn a_delivery_s_alert_is_announced_where_the_choice_takes_it() {
    let payload = json!({
        "action": "created",
        "alert": listed_alert(9, "open"),
        "repository": { "id": 7, "name": "r", "full_name": "o/r", "html_url": PAGE },
    })
    .to_string();
    let every = SourceOptions::default().alert_choice(Forge::GitHub);
    let delivery = (
        Delivery::GitHub(Scanner::Dependabot),
        "https://api.github.com",
    );
    let found = delivered(delivery, payload.as_bytes(), &every).unwrap();
    assert_eq!(found.path, "o/r");
    assert_eq!(found.repository, repository());
    assert_eq!(
        found.event.unwrap().identity().as_str(),
        "github.com/7/alert/dependency-scanning/9/created"
    );
    let narrow = SourceOptions {
        alerts: Some(crate::Alerts {
            actions: Some(vec![AlertAction::Fixed]),
            ..crate::Alerts::default()
        }),
        ..SourceOptions::default()
    }
    .alert_choice(Forge::GitHub);
    assert_eq!(
        delivered(delivery, payload.as_bytes(), &narrow)
            .unwrap()
            .event,
        None
    );
    assert!(delivered(delivery, b"{}", &every).is_err());
}
