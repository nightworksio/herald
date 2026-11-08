use herald_core::Command;

use super::*;
use crate::interactions::{Interactions, SESSION_DOMAIN};

const HOOK: &str = "https://hooks.slack.com/commands/T1/2/abc";

fn interacting() -> Interacting {
    let interactions: Interactions =
        serde_json::from_str(r#"{"via": "socket", "app": {"secret": "HERALD_SLACK_APP"}}"#)
            .unwrap();
    Interacting::new(interactions, None)
}

fn destination() -> DestinationName {
    DestinationName::new("slack")
}

fn envelope(kind: &str, payload: &Value) -> String {
    json!({ "envelope_id": "e1", "type": kind, "payload": payload }).to_string()
}

fn command(text: &str) -> Value {
    json!({
        "command": "/herald",
        "text": text,
        "channel_id": "C0123456789",
        "user_id": "U0123456789",
        "response_url": HOOK,
        "is_enterprise_install": false,
    })
}

#[test]
fn a_connection_is_opened_with_the_app_level_token_at_the_address_slack_answers() {
    let opening = interacting().opening(&Secret::new("HERALD_SLACK_APP", "xapp-1"));
    assert_eq!(opening.method, Method::Post);
    assert_eq!(
        opening.url.expose(),
        "https://slack.com/api/apps.connections.open"
    );
    let authorization = opening
        .headers
        .iter()
        .find(|(name, _)| name == "Authorization")
        .map(|(_, value)| value.expose());
    assert_eq!(authorization.as_deref(), Some("Bearer xapp-1"));
    let answer = |body: &str| Response {
        status: 200,
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
    };
    assert_eq!(
        opened(
            &answer(r#"{"ok": true, "url": "wss://wss-primary.slack.com/link"}"#),
            &[SESSION_DOMAIN]
        ),
        Ok("wss://wss-primary.slack.com/link".to_owned())
    );
    assert_eq!(
        opened(
            &answer(r#"{"ok": true, "url": "wss://wss.evil.example/link?ticket=1"}"#),
            &[SESSION_DOMAIN]
        ),
        Err("Slack opened a Socket Mode connection on a host outside slack.com; add its domain to the destination's interactions domains to take it".to_owned())
    );
    assert_eq!(
        opened(
            &answer(r#"{"ok": true, "url": "wss://wss.proxy.example/link"}"#),
            &[SESSION_DOMAIN, "proxy.example"]
        ),
        Ok("wss://wss.proxy.example/link".to_owned())
    );
    assert_eq!(
        opened(
            &answer(r#"{"ok": false, "error": "invalid_auth"}"#),
            &[SESSION_DOMAIN]
        ),
        Err("Slack opened no Socket Mode connection: invalid_auth".to_owned())
    );
    assert_eq!(
        opened(
            &answer(r#"{"ok": true, "url": "https://elsewhere.example"}"#),
            &[SESSION_DOMAIN]
        ),
        Err("Slack opened no Socket Mode connection: it answered with no address".to_owned())
    );
}

#[test]
fn a_slash_command_is_acknowledged_and_handed_over() {
    let enveloped = interacting().envelope(
        &destination(),
        &envelope("slash_commands", &command("latest gate")),
    );
    assert_eq!(
        enveloped.acknowledgement.as_deref(),
        Some(r#"{"envelope_id":"e1"}"#)
    );
    let Some(Received::Asked { asked, replying }) = enveloped.received else {
        panic!("a command is handed over");
    };
    assert_eq!(asked.command, Command::Latest);
    assert_eq!(asked.project.as_deref(), Some("gate"));
    assert_eq!(replying.url.expose(), HOOK);
    assert!(!enveloped.reconnect);
}

#[test]
fn a_command_herald_cannot_read_is_acknowledged_with_its_usage() {
    let enveloped = interacting().envelope(
        &destination(),
        &envelope("slash_commands", &command("deploy")),
    );
    let acknowledgement: Value =
        serde_json::from_str(enveloped.acknowledgement.as_deref().unwrap()).unwrap();
    assert_eq!(acknowledgement["envelope_id"], "e1");
    assert_eq!(acknowledgement["payload"]["response_type"], "ephemeral");
    assert_eq!(enveloped.received, None);
}

#[test]
fn a_button_press_is_handed_over_and_other_envelopes_only_acknowledged() {
    let press = json!({
        "type": "block_actions",
        "user": { "id": "U0123456789" },
        "channel": { "id": "C0123456789" },
        "response_url": HOOK,
        "actions": [{ "action_id": "herald_subscribe", "value": "gate" }],
    });
    let pressed = interacting().envelope(&destination(), &envelope("interactive", &press));
    assert!(matches!(pressed.received, Some(Received::Asked { asked, .. }) if asked.toggles));
    for (kind, payload) in [
        ("events_api", json!({})),
        ("interactive", json!({ "type": "view_submission" })),
        ("slash_commands", json!({ "command": "/other" })),
        ("slash_commands", json!({ "text": "latest" })),
    ] {
        let enveloped = interacting().envelope(&destination(), &envelope(kind, &payload));
        assert_eq!(
            enveloped.acknowledgement.as_deref(),
            Some(r#"{"envelope_id":"e1"}"#),
            "{kind}"
        );
        assert_eq!(enveloped.received, None, "{kind}");
    }
}

#[test]
fn a_disconnect_reconnects_a_greeting_is_noted_and_noise_asks_nothing() {
    let disconnect = interacting().envelope(
        &destination(),
        r#"{"type": "disconnect", "reason": "refresh_requested"}"#,
    );
    assert!(disconnect.reconnect);
    assert!(!disconnect.greeted);
    assert_eq!(disconnect.acknowledgement, None);
    let quiet = Enveloped {
        acknowledgement: None,
        received: None,
        reconnect: false,
        greeted: false,
    };
    assert_eq!(
        interacting().envelope(&destination(), r#"{"type": "hello"}"#),
        Enveloped {
            greeted: true,
            ..quiet.clone()
        }
    );
    assert_eq!(interacting().envelope(&destination(), "not json"), quiet);
    assert_eq!(
        interacting().envelope(&destination(), r#"{"type": "slash_commands"}"#),
        quiet
    );
}
