//! An owner's members' alerts, read beside what GraphQL reads of each, and
//! every member polled each round where alerts are announced.

use herald_core::{Identity, Signal};

use super::*;

fn alerted(transport: Option<Transport>) -> Owned {
    owned(
        Forge::GitHub,
        &SourceOptions {
            token: Some(herald_core::SecretRef {
                secret: "TOKEN".to_owned(),
            }),
            signals: Some(vec![Signal::Release, Signal::Alert]),
            alerts: Some(crate::Alerts {
                tools: Some(vec![crate::Tool::SecretScanning]),
                ..crate::Alerts::default()
            }),
            transport,
            ..SourceOptions::default()
        },
    )
}

fn alerts() -> Response {
    let alert = json!({
        "number": 2, "state": "open",
        "html_url": "https://github.com/lemonfiber/cli/security/secret-scanning/2",
        "created_at": "2026-11-18T08:00:00Z",
        "secret_type": "slack_api_token",
    });
    answer(200, &json!([alert]), "\"alerts\"", false)
}

#[test]
fn a_member_polled_through_graphql_reads_its_alerts_beside_and_keeps_their_validators() {
    let source = alerted(None);
    let cli = member();
    let cursor = Cursor {
        seen: vec![Identity::new("github.com/9/v0")],
        ..Cursor::default()
    };
    let http = ScriptedHttp::new(vec![released(), alerts()]);
    let polled = pollster::block_on(source.polled(
        &http,
        &[(&cli, Some(&cursor))],
        &Rounded::Nothing,
        ROUND,
    ));
    let Ok(Some(found)) = &polled[0] else {
        panic!("{polled:?}");
    };
    let identities: Vec<&str> = found
        .events
        .iter()
        .map(|event| event.identity().as_str())
        .collect();
    assert_eq!(identities, ["github.com/9/alert/secret-scanning/2/created"]);
    assert_eq!(
        found.listings["alert/secret-scanning"].etag.as_deref(),
        Some("\"alerts\"")
    );
    assert_eq!(
        http.asked()[1].0,
        "https://api.github.com/repos/lemonfiber/cli/secret-scanning/alerts?sort=updated&direction=desc&per_page=30"
    );
    let quiet = ScriptedHttp::new(vec![released(), unchanged()]);
    let polled = pollster::block_on(source.polled(
        &quiet,
        &[(&cli, Some(&cursor))],
        &Rounded::Nothing,
        ROUND,
    ));
    assert!(matches!(polled[0], Ok(None)), "{polled:?}");
}

#[test]
fn an_owner_announcing_alerts_polls_every_member_whatever_its_events_say() {
    let source = alerted(Some(Transport::Events));
    let nothing = ScriptedHttp::new(Vec::new());
    assert!(pollster::block_on(source.changed(&nothing, Owner::Organization, None)).is_none());
    assert_eq!(nothing.asked(), Vec::new());
}
