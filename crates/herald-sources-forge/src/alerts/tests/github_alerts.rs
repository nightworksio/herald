use serde_json::{Value, json};

use super::super::github::{Scanner, delivered, listed, listing_path};
use super::*;

fn hooked(scanner: Scanner, action: &str, alert: &Value) -> Option<ForgeAlert> {
    let payload = json!({
        "action": action,
        "alert": alert,
        "repository": { "id": 7, "name": "r", "full_name": "o/r", "html_url": PAGE },
    });
    delivered(scanner, payload.to_string().as_bytes())
        .unwrap()
        .2
}

fn raised(extra: &Value) -> Value {
    let mut alert = json!({
        "number": 5,
        "state": "open",
        "html_url": format!("{PAGE}/security/5"),
        "created_at": "2026-11-18T08:00:00Z",
        "updated_at": "2026-11-18T10:00:00Z",
    });
    alert
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    alert
}

#[test]
fn each_scanner_s_event_action_reads_as_what_the_alert_did() {
    let cases = [
        (Scanner::CodeScanning, "created", Some(AlertAction::Created)),
        (
            Scanner::CodeScanning,
            "reopened",
            Some(AlertAction::Reopened),
        ),
        (
            Scanner::CodeScanning,
            "reopened_by_user",
            Some(AlertAction::Reopened),
        ),
        (Scanner::CodeScanning, "fixed", Some(AlertAction::Fixed)),
        (
            Scanner::CodeScanning,
            "closed_by_user",
            Some(AlertAction::Dismissed),
        ),
        (Scanner::CodeScanning, "appeared_in_branch", None),
        (
            Scanner::Dependabot,
            "reintroduced",
            Some(AlertAction::Reopened),
        ),
        (
            Scanner::Dependabot,
            "auto_reopened",
            Some(AlertAction::Reopened),
        ),
        (Scanner::Dependabot, "fixed", Some(AlertAction::Fixed)),
        (
            Scanner::Dependabot,
            "dismissed",
            Some(AlertAction::Dismissed),
        ),
        (
            Scanner::Dependabot,
            "auto_dismissed",
            Some(AlertAction::Dismissed),
        ),
        (Scanner::Dependabot, "closed_by_user", None),
        (Scanner::SecretScanning, "fixed", None),
        (Scanner::SecretScanning, "dismissed", None),
        (Scanner::SecretScanning, "validated", None),
    ];
    for (scanner, action, read) in cases {
        let found = hooked(scanner, action, &raised(&json!({})));
        assert_eq!(
            found.map(|alert| alert.action),
            read,
            "{scanner:?} {action}"
        );
    }
    let resolved = |resolution: &str| {
        hooked(
            Scanner::SecretScanning,
            "resolved",
            &raised(&json!({ "state": "resolved", "resolution": resolution, "resolved_at": "2026-11-18T11:00:00Z" })),
        )
        .unwrap()
    };
    let revoked = resolved("revoked");
    assert_eq!(
        (revoked.action, revoked.resolution.as_deref()),
        (AlertAction::Fixed, Some("revoked"))
    );
    let kept = resolved("false_positive");
    assert_eq!(
        (kept.action, kept.resolution.as_deref()),
        (AlertAction::Dismissed, Some("false_positive"))
    );
    assert_eq!(kept.occurred, "2026-11-18T11:00:00Z".parse().unwrap());
}

#[test]
fn a_code_scanning_or_dependabot_alert_is_read_for_what_it_found() {
    let code = hooked(
        Scanner::CodeScanning,
        "created",
        &raised(&json!({
            "rule": { "id": "js/xss", "severity": "error", "security_severity_level": "high", "description": "XSS" },
            "most_recent_instance": { "location": { "path": "src/a.js" } },
        })),
    )
    .unwrap();
    assert_eq!(
        (
            code.tool,
            code.rule.as_deref(),
            code.severity,
            code.summary.as_deref(),
            code.path.as_deref()
        ),
        (
            Tool::CodeScanning,
            Some("js/xss"),
            Some(Severity::High),
            Some("XSS"),
            Some("src/a.js")
        )
    );
    assert_eq!(code.occurred, "2026-11-18T08:00:00Z".parse().unwrap());
    let ruled = hooked(
        Scanner::CodeScanning,
        "reopened",
        &raised(&json!({ "rule": { "severity": "warning" } })),
    )
    .unwrap();
    assert_eq!(ruled.severity, Some(Severity::Warning));
    assert_eq!(ruled.occurred, "2026-11-18T10:00:00Z".parse().unwrap());
    let dependency = hooked(
        Scanner::Dependabot,
        "dismissed",
        &raised(&json!({
            "state": "dismissed",
            "dismissed_at": "2026-11-18T12:00:00Z",
            "dependency": { "package": { "ecosystem": "npm", "name": "left-pad" }, "manifest_path": "package.json" },
            "security_advisory": { "ghsa_id": "GHSA-x", "severity": "medium", "summary": "Bad pad" },
            "most_recent_instance": { "location": { "path": "ignored.js" } },
        })),
    )
    .unwrap();
    assert_eq!(
        (
            dependency.tool,
            dependency.package.as_deref(),
            dependency.ecosystem.as_deref(),
            dependency.path.as_deref(),
            dependency.rule.as_deref(),
            dependency.severity,
            dependency.summary.as_deref(),
        ),
        (
            Tool::DependencyScanning,
            Some("left-pad"),
            Some("npm"),
            Some("package.json"),
            Some("GHSA-x"),
            Some(Severity::Medium),
            Some("Bad pad"),
        )
    );
    assert_eq!(dependency.occurred, "2026-11-18T12:00:00Z".parse().unwrap());
}

#[test]
fn an_alert_naming_little_is_read_as_it_is_and_a_secret_never_is() {
    let unpackaged = hooked(
        Scanner::Dependabot,
        "auto_dismissed",
        &raised(&json!({ "auto_dismissed_at": "2026-11-18T13:00:00Z", "dependency": {} })),
    )
    .unwrap();
    assert_eq!((unpackaged.package, unpackaged.path), (None, None));
    assert_eq!(unpackaged.occurred, "2026-11-18T13:00:00Z".parse().unwrap());
    let secret = hooked(
        Scanner::SecretScanning,
        "created",
        &raised(&json!({
            "secret_type": "github_personal_access_token",
            "secret_type_display_name": "GitHub Personal Access Token",
            "secret": "ghp_hunter2",
        })),
    )
    .unwrap();
    assert_eq!(
        (
            secret.tool,
            secret.rule.as_deref(),
            secret.summary.as_deref(),
            secret.severity
        ),
        (
            Tool::SecretScanning,
            Some("github_personal_access_token"),
            Some("GitHub Personal Access Token"),
            None
        )
    );
    let event = secret.event("alerts", &repository()).unwrap();
    let shown = format!(
        "{:?}{:?}{}",
        event.attributes(),
        event.links(),
        event.title()
    );
    assert!(!shown.contains("hunter2"), "{shown}");
    let closed = hooked(
        Scanner::CodeScanning,
        "fixed",
        &json!({ "number": 5, "state": "fixed", "html_url": PAGE, "created_at": "2026-11-18T08:00:00Z" }),
    )
    .unwrap();
    assert_eq!(closed.occurred, "2026-11-18T08:00:00Z".parse().unwrap());
    let updated = hooked(
        Scanner::CodeScanning,
        "fixed",
        &raised(&json!({ "state": "fixed" })),
    )
    .unwrap();
    assert_eq!(updated.occurred, "2026-11-18T10:00:00Z".parse().unwrap());
}

#[test]
fn a_listing_reads_each_alert_by_its_state_within_its_bytes() {
    let body = json!([
        raised(&json!({ "number": 1 })),
        raised(&json!({ "number": 2, "state": "dismissed" })),
        raised(&json!({ "number": 3, "state": "auto_dismissed" })),
        raised(&json!({ "number": 4, "state": "resolved", "resolution": "revoked" })),
        raised(&json!({ "number": 5, "state": "resolved", "resolution": "used_in_tests" })),
        raised(&json!({ "number": 6, "state": "fixed" })),
        raised(&json!({ "number": 7, "state": "unknown" })),
    ])
    .to_string();
    let found = listed(Scanner::SecretScanning, 1 << 20, body.as_bytes()).unwrap();
    let read: Vec<(u64, AlertAction)> = found
        .iter()
        .map(|alert| (alert.number, alert.action))
        .collect();
    assert_eq!(
        read,
        [
            (1, AlertAction::Created),
            (2, AlertAction::Dismissed),
            (3, AlertAction::Dismissed),
            (4, AlertAction::Fixed),
            (5, AlertAction::Dismissed),
            (6, AlertAction::Fixed),
        ]
    );
    assert!(listed(Scanner::SecretScanning, 8, body.as_bytes()).is_err());
    assert_eq!(
        listing_path(Scanner::Dependabot, 50),
        "/dependabot/alerts?sort=updated&direction=desc&per_page=50"
    );
    let named: Vec<Option<Scanner>> = [
        "code_scanning_alert",
        "dependabot_alert",
        "secret_scanning_alert",
        "secret_scanning_alert_location",
    ]
    .into_iter()
    .map(Scanner::hooked)
    .collect();
    assert_eq!(
        named,
        [
            Some(Scanner::CodeScanning),
            Some(Scanner::Dependabot),
            Some(Scanner::SecretScanning),
            None
        ]
    );
    let read: Vec<(&str, &str, Tool)> = Scanner::ALL
        .into_iter()
        .map(|scanner| (scanner.segment(), scanner.listed_what(), scanner.tool()))
        .collect();
    assert_eq!(
        read,
        [
            (
                "code-scanning",
                "GitHub's code scanning alerts",
                Tool::CodeScanning
            ),
            (
                "dependabot",
                "GitHub's Dependabot alerts",
                Tool::DependencyScanning
            ),
            (
                "secret-scanning",
                "GitHub's secret scanning alerts",
                Tool::SecretScanning
            ),
        ]
    );
    let hooked_what: Vec<&str> = Scanner::ALL.into_iter().map(Scanner::hooked_what).collect();
    assert_eq!(
        hooked_what,
        [
            "GitHub's code scanning alert event",
            "GitHub's Dependabot alert event",
            "GitHub's secret scanning alert event"
        ]
    );
}
