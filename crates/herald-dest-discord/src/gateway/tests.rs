use herald_core::Command;

use super::*;
use crate::interactions::{Interactions, SESSION_DOMAIN};

const APPLICATION: &str = "123456789012345678";

fn interacting() -> Interacting {
    let interactions: Interactions = serde_json::from_str(&format!(
        r#"{{"application": "{APPLICATION}", "via": "gateway"}}"#
    ))
    .unwrap();
    Interacting::new(interactions, Some(token()), None)
}

fn token() -> Secret {
    Secret::new("HERALD_DISCORD_TOKEN", "bot.token-1")
}

fn destination() -> DestinationName {
    DestinationName::new("discord")
}

fn received(gateway: &mut Gateway, frame: &Value) -> Vec<Effect> {
    gateway.received(&interacting(), &destination(), &frame.to_string())
}

fn sent(effect: &Effect) -> Value {
    let Effect::Send(text) = effect else {
        panic!("{effect:?} sends nothing");
    };
    serde_json::from_str(&text.expose()).unwrap()
}

fn hello(gateway: &mut Gateway) -> Vec<Effect> {
    received(
        gateway,
        &json!({ "op": 10, "d": { "heartbeat_interval": 41250 } }),
    )
}

fn ready(gateway: &mut Gateway) {
    let effects = received(
        gateway,
        &json!({ "op": 0, "s": 1, "t": "READY", "d": {
            "session_id": "session",
            "resume_gateway_url": "wss://resume.discord.gg/",
        }}),
    );
    assert_eq!(effects, [Effect::Ready]);
}

#[test]
fn a_session_identifies_on_hello_and_beats_as_often_as_discord_says() {
    let mut gateway = Gateway::new(token(), &[SESSION_DOMAIN]);
    assert_eq!(gateway.url(), GATEWAY);
    let effects = hello(&mut gateway);
    assert_eq!(effects[0], Effect::Beat(SignedDuration::from_millis(41250)));
    assert_eq!(
        sent(&effects[1]),
        json!({ "op": 2, "d": {
            "token": "bot.token-1",
            "intents": 0,
            "properties": { "os": "linux", "browser": "herald", "device": "herald" },
        }})
    );
    let Effect::Send(identify) = &effects[1] else {
        panic!("it identifies");
    };
    assert!(!identify.redacted().contains("bot.token-1"));
    let silent = received(&mut gateway, &json!({ "op": 10, "d": {} }));
    assert_eq!(silent[0], Effect::Beat(SignedDuration::from_secs(41)));
}

#[test]
fn a_beat_carries_the_last_sequence_and_an_unacknowledged_beat_reconnects() {
    let mut gateway = Gateway::new(token(), &[SESSION_DOMAIN]);
    hello(&mut gateway);
    assert_eq!(sent(&gateway.beat()), json!({ "op": 1, "d": null }));
    ready(&mut gateway);
    assert_eq!(gateway.beat(), Effect::Reconnect { resume: true });
    let mut acknowledged = Gateway::new(token(), &[SESSION_DOMAIN]);
    hello(&mut acknowledged);
    ready(&mut acknowledged);
    assert_eq!(sent(&acknowledged.beat()), json!({ "op": 1, "d": 1 }));
    assert_eq!(received(&mut acknowledged, &json!({ "op": 11 })), []);
    assert!(matches!(acknowledged.beat(), Effect::Send(_)));
    let asked = received(&mut acknowledged, &json!({ "op": 1 }));
    assert_eq!(sent(&asked[0]), json!({ "op": 1, "d": 1 }));
}

#[test]
fn a_session_resumes_only_under_its_domains() {
    let mut gateway = Gateway::new(token(), &[SESSION_DOMAIN]);
    hello(&mut gateway);
    received(
        &mut gateway,
        &json!({ "op": 0, "s": 1, "t": "READY", "d": {
            "session_id": "session",
            "resume_gateway_url": "wss://resume.evil.example/",
        }}),
    );
    received(&mut gateway, &json!({ "op": 7 }));
    assert_eq!(gateway.url(), GATEWAY);
    let mut proxied = Gateway::new(token(), &[SESSION_DOMAIN, "proxy.example"]);
    hello(&mut proxied);
    received(
        &mut proxied,
        &json!({ "op": 0, "s": 1, "t": "READY", "d": {
            "session_id": "session",
            "resume_gateway_url": "wss://resume.proxy.example/",
        }}),
    );
    received(&mut proxied, &json!({ "op": 7 }));
    assert_eq!(
        proxied.url(),
        "wss://resume.proxy.example/?v=10&encoding=json"
    );
}

#[test]
fn a_session_told_to_reconnect_resumes_where_discord_said() {
    let mut gateway = Gateway::new(token(), &[SESSION_DOMAIN]);
    hello(&mut gateway);
    ready(&mut gateway);
    assert_eq!(
        received(&mut gateway, &json!({ "op": 7 })),
        [Effect::Reconnect { resume: true }]
    );
    assert_eq!(gateway.url(), "wss://resume.discord.gg/?v=10&encoding=json");
    let effects = hello(&mut gateway);
    assert_eq!(
        sent(&effects[1]),
        json!({ "op": 6, "d": { "token": "bot.token-1", "session_id": "session", "seq": 1 } })
    );
    assert_eq!(gateway.url(), GATEWAY);
}

#[test]
fn an_invalid_session_identifies_afresh_unless_discord_says_it_resumes() {
    let mut gateway = Gateway::new(token(), &[SESSION_DOMAIN]);
    hello(&mut gateway);
    ready(&mut gateway);
    assert_eq!(
        received(&mut gateway, &json!({ "op": 9, "d": false })),
        [Effect::Reconnect { resume: false }]
    );
    assert_eq!(sent(&hello(&mut gateway)[1])["op"], 2);
    ready(&mut gateway);
    assert_eq!(
        received(&mut gateway, &json!({ "op": 9, "d": true })),
        [Effect::Reconnect { resume: true }]
    );
    let mut fresh = Gateway::new(token(), &[SESSION_DOMAIN]);
    assert_eq!(fresh.closed(None), Effect::Reconnect { resume: false });
}

#[test]
fn a_close_reconnecting_cannot_help_stops_and_any_other_resumes() {
    let mut gateway = Gateway::new(token(), &[SESSION_DOMAIN]);
    hello(&mut gateway);
    ready(&mut gateway);
    for code in [4004, 4010, 4011, 4012, 4013, 4014] {
        assert!(
            matches!(gateway.closed(Some(code)), Effect::Stop(reason) if reason.contains(&code.to_string()))
        );
    }
    assert_eq!(
        gateway.closed(Some(4000)),
        Effect::Reconnect { resume: true }
    );
    assert_eq!(gateway.closed(None), Effect::Reconnect { resume: true });
}

#[test]
fn a_session_that_cannot_be_taken_up_again_starts_afresh_and_is_ready_once_resumed() {
    for code in [4007, 4009] {
        let mut gateway = Gateway::new(token(), &[SESSION_DOMAIN]);
        hello(&mut gateway);
        ready(&mut gateway);
        assert_eq!(
            gateway.closed(Some(code)),
            Effect::Reconnect { resume: false },
            "{code}"
        );
        assert_eq!(gateway.url(), GATEWAY);
    }
    let mut gateway = Gateway::new(token(), &[SESSION_DOMAIN]);
    hello(&mut gateway);
    ready(&mut gateway);
    gateway.closed(None);
    assert_eq!(
        received(
            &mut gateway,
            &json!({ "op": 0, "s": 2, "t": "RESUMED", "d": {} })
        ),
        [Effect::Ready]
    );
}

#[test]
fn an_interaction_dispatched_is_handed_over_and_others_pass() {
    let mut gateway = Gateway::new(token(), &[SESSION_DOMAIN]);
    let interaction = json!({ "op": 0, "s": 5, "t": "INTERACTION_CREATE", "d": {
        "id": "900",
        "token": "tok",
        "type": 2,
        "application_id": APPLICATION,
        "channel_id": "323456789012345678",
        "user": { "id": "423456789012345678" },
        "data": { "name": "latest" },
    }});
    let effects = received(&mut gateway, &interaction);
    let [Effect::Received(handed)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    let Received::Asked { asked, .. } = handed.as_ref() else {
        panic!("{handed:?}");
    };
    assert_eq!(asked.command, Command::Latest);
    let mut other = interaction.clone();
    other["d"]["application_id"] = json!("999999999999999999");
    assert_eq!(received(&mut gateway, &other), []);
    let mut broken = interaction;
    broken["d"].as_object_mut().unwrap().remove("channel_id");
    assert_eq!(received(&mut gateway, &broken), []);
    assert_eq!(
        received(
            &mut gateway,
            &json!({ "op": 0, "t": "GUILD_CREATE", "d": {} })
        ),
        []
    );
    assert_eq!(received(&mut gateway, &json!({ "op": 3 })), []);
    assert_eq!(
        gateway.received(&interacting(), &destination(), "not json"),
        []
    );
}

#[test]
fn a_frame_without_the_token_is_written_as_it_is() {
    assert_eq!(framed(&json!({ "a": 1 }), &token()).expose(), r#"{"a":1}"#);
}
