use serde_json::{Value, json};

use super::super::gitlab::{delivered, listed, listing};
use super::*;

const VULNERABILITIES: &str = "https://gitlab.example/group/web/-/security/vulnerabilities";

/// A vulnerability, as GitLab's GraphQL listing names it.
pub(super) fn node(state: &str, number: u64) -> Value {
    json!({
        "title": "REXML DoS",
        "state": state,
        "severity": "HIGH",
        "reportType": "DEPENDENCY_SCANNING",
        "detectedAt": "2026-11-18T08:00:00Z",
        "confirmedAt": null,
        "resolvedAt": "2026-11-18T09:00:00Z",
        "dismissedAt": null,
        "updatedAt": "2026-11-18T10:00:00Z",
        "webUrl": format!("{VULNERABILITIES}/{number}"),
        "primaryIdentifier": { "name": "CVE-2024-41123" },
        "location": { "file": "Gemfile.lock", "dependency": { "package": { "name": "rexml" } } },
    })
}

fn listing_of(nodes: &[Value]) -> Vec<ForgeAlert> {
    let body = json!({ "data": { "project": { "vulnerabilities": { "nodes": nodes } } } });
    listed(1 << 20, body.to_string().as_bytes()).unwrap()
}

fn with(mut node: Value, extra: &Value) -> Value {
    node.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    node
}

#[test]
fn each_report_type_and_state_reads_as_a_tool_and_an_action() {
    let types = [
        ("SAST", Tool::CodeScanning),
        ("dependency_scanning", Tool::DependencyScanning),
        ("SECRET_DETECTION", Tool::SecretScanning),
        ("CONTAINER_SCANNING", Tool::ContainerScanning),
        ("CLUSTER_IMAGE_SCANNING", Tool::ContainerScanning),
        ("CONTAINER_SCANNING_FOR_REGISTRY", Tool::ContainerScanning),
        ("DAST", Tool::Dast),
        ("COVERAGE_FUZZING", Tool::Fuzzing),
        ("API_FUZZING", Tool::Fuzzing),
        ("GENERIC", Tool::Other),
    ];
    for (reported, tool) in types {
        let found = listing_of(&[with(
            node("DETECTED", 1),
            &json!({ "reportType": reported }),
        )]);
        assert_eq!(found[0].tool, tool, "{reported}");
    }
    let unreported = listing_of(&[with(node("DETECTED", 1), &json!({ "reportType": null }))]);
    assert_eq!(unreported[0].tool, Tool::Other);
    let states = listing_of(&[
        node("DETECTED", 1),
        with(
            node("CONFIRMED", 2),
            &json!({ "confirmedAt": "2026-11-18T08:30:00Z" }),
        ),
        node("resolved", 3),
        with(
            node("DISMISSED", 4),
            &json!({ "dismissedAt": "2026-11-18T09:30:00Z" }),
        ),
        node("PENDING_VERIFICATION", 5),
        node("CONFIRMED", 6),
    ]);
    let read: Vec<(u64, AlertAction, String)> = states
        .iter()
        .map(|alert| (alert.number, alert.action, alert.occurred.to_string()))
        .collect();
    assert_eq!(
        read,
        [
            (1, AlertAction::Created, "2026-11-18T08:00:00Z".to_owned()),
            (2, AlertAction::Confirmed, "2026-11-18T08:30:00Z".to_owned()),
            (3, AlertAction::Fixed, "2026-11-18T09:00:00Z".to_owned()),
            (4, AlertAction::Dismissed, "2026-11-18T09:30:00Z".to_owned()),
            (6, AlertAction::Confirmed, "2026-11-18T10:00:00Z".to_owned()),
        ]
    );
    let bare = listing_of(&[with(
        node("CONFIRMED", 7),
        &json!({ "updatedAt": null, "severity": null, "location": null, "primaryIdentifier": null }),
    )]);
    assert_eq!(bare[0].occurred, "2026-11-18T08:00:00Z".parse().unwrap());
    assert_eq!(
        (
            bare[0].severity,
            bare[0].path.as_deref(),
            bare[0].rule.as_deref()
        ),
        (None, None, None)
    );
}

#[test]
fn a_vulnerability_is_read_for_what_it_found_where_it_was_found() {
    let found = &listing_of(&[node("DETECTED", 12)])[0];
    assert_eq!(
        (
            found.number,
            found.severity,
            found.rule.as_deref(),
            found.summary.as_deref(),
            found.package.as_deref(),
            found.path.as_deref(),
            found.ecosystem.as_deref(),
            found.resolution.as_deref(),
        ),
        (
            12,
            Some(Severity::High),
            Some("CVE-2024-41123"),
            Some("REXML DoS"),
            Some("rexml"),
            Some("Gemfile.lock"),
            None,
            None,
        )
    );
    let image = &listing_of(&[with(
        node("DETECTED", 1),
        &json!({ "location": { "image": "registry.example/web:1", "dependency": { "package": {} } } }),
    )])[0];
    assert_eq!(
        (image.path.as_deref(), image.package.as_deref()),
        (Some("registry.example/web:1"), None)
    );
    for url in [
        "https://gitlab.example/group/web/-/issues/1",
        "https://gitlab.example/group/web/-/security/vulnerabilities/x",
        "nowhere/-/security/vulnerabilities/1",
    ] {
        let body = json!({ "data": { "project": { "vulnerabilities": { "nodes": [with(node("DETECTED", 1), &json!({ "webUrl": url }))] } } } });
        let refused = listed(1 << 20, body.to_string().as_bytes()).unwrap_err();
        let said = refused.to_string();
        assert!(
            said.contains(super::super::gitlab::UNPAGED),
            "{url}: {said}"
        );
        assert!(!said.contains(url), "{url}: {said}");
    }
}

#[test]
fn a_listing_without_vulnerabilities_names_the_tier_they_need() {
    for body in [
        json!({ "data": { "project": null }, "errors": [{ "message": "no" }] }),
        json!({ "data": { "project": { "vulnerabilities": null } } }),
        json!({ "data": null }),
    ] {
        let refused = listed(1 << 20, body.to_string().as_bytes()).unwrap_err();
        assert!(
            refused.to_string().contains("need GitLab Ultimate"),
            "{refused}"
        );
    }
    assert!(listed(4, b"{\"data\": null}").is_err());
    let token = crate::Token::Named(herald_ports::Secret::new("GITLAB_TOKEN", "glpat"));
    let request = listing(
        "https://gitlab.example/api/graphql",
        ("group/web", Some(&token)),
        (20, 4096),
    );
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(
        body["variables"],
        json!({ "path": "group/web", "page": 20 })
    );
    assert!(
        body["query"]
            .as_str()
            .unwrap()
            .starts_with("query($path: ID!, $page: Int!) { project(fullPath: $path)")
    );
    assert!(
        request
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization")
                || name.eq_ignore_ascii_case("private-token"))
    );
}

fn hook(url: &str, state: &str) -> Vec<u8> {
    json!({
        "object_kind": "vulnerability",
        "object_attributes": {
            "url": url,
            "title": "REXML DoS vulnerability",
            "state": state,
            "project_id": 50,
            "location": { "file": "Gemfile.lock", "dependency": { "package": { "name": "rexml" }, "version": "3.3.1" } },
            "severity": "high",
            "identifiers": [{ "name": "Gemnasium-29dce398" }, { "name": "CVE-2024-41123" }],
            "report_type": "dependency_scanning",
            "confirmed_at": "2025-01-08T00:46:14.413Z",
            "dismissed_at": null,
            "resolved_at": null,
            "created_at": "2025-01-08T00:46:14.413Z",
            "updated_at": "2025-01-08T00:46:14.413Z"
        }
    })
    .to_string()
    .into_bytes()
}

#[test]
fn a_vulnerability_event_names_its_project_by_its_page_under_the_instance() {
    let (path, repository, alert) = delivered(
        "https://example.com",
        &hook(
            "https://example.com/flightjs/Flight/-/security/vulnerabilities/1",
            "confirmed",
        ),
    )
    .unwrap();
    assert_eq!(path, "flightjs/Flight");
    assert_eq!(
        repository,
        Repository {
            id: 50,
            name: "Flight".to_owned(),
            page: Some("https://example.com/flightjs/Flight".to_owned()),
        }
    );
    let alert = alert.unwrap();
    assert_eq!(
        (
            alert.number,
            alert.action,
            alert.rule.as_deref(),
            alert.tool
        ),
        (
            1,
            AlertAction::Confirmed,
            Some("Gemnasium-29dce398"),
            Tool::DependencyScanning
        )
    );
    let (path, _, _) = delivered(
        "https://Git.Example/gitlab/",
        &hook(
            "https://git.example/gitlab/a/b/c/-/security/vulnerabilities/2",
            "detected",
        ),
    )
    .unwrap();
    assert_eq!(path, "a/b/c");
    let (_, _, unread) = delivered(
        "https://example.com",
        &hook(
            "https://example.com/a/b/-/security/vulnerabilities/3",
            "pending_verification",
        ),
    )
    .unwrap();
    assert_eq!(unread, None);
    for (web, url) in [
        (
            "https://example.com",
            "https://elsewhere.example/a/b/-/security/vulnerabilities/1",
        ),
        (
            "https://example.com/gitlab",
            "https://example.com/a/b/-/security/vulnerabilities/1",
        ),
        (
            "https://example.com",
            "https://example.com/-/security/vulnerabilities/1",
        ),
        (
            "https://example.com/a",
            "https://example.com/ab/c/-/security/vulnerabilities/1",
        ),
        (
            "https://example.com",
            "https://examplf.com/a/b/-/security/vulnerabilities/1",
        ),
        (
            "https://example.com",
            "https://example.com//-/security/vulnerabilities/1",
        ),
    ] {
        assert!(
            delivered(web, &hook(url, "detected")).is_err(),
            "{web} {url}"
        );
    }
    assert!(
        delivered(
            "https://example.com",
            &hook("https://example.com/a/b/-/issues/1", "detected")
        )
        .is_err()
    );
    assert!(delivered("https://example.com", b"{}").is_err());
}

#[test]
fn an_instance_s_web_root_is_its_api_without_api_v4() {
    assert_eq!(
        crate::gitlab::web("https://gitlab.com/api/v4"),
        "https://gitlab.com"
    );
    assert_eq!(
        crate::gitlab::web("https://git.example/gitlab/api/v4/"),
        "https://git.example/gitlab"
    );
    assert_eq!(
        crate::gitlab::web("https://git.example/other"),
        "https://git.example/other"
    );
}
