use herald_core::Replies;

use super::*;

const HOOK: &str = "https://hooks.slack.com/commands/T1/2/abc";

fn interactions(extra: &str) -> Interactions {
    serde_json::from_str(&format!(
        r#"{{"signing": {{"secret": "HERALD_SLACK_SIGNING"}}{extra}}}"#
    ))
    .unwrap()
}

fn interacting(extra: &str) -> Interacting {
    Interacting::new(interactions(extra), None)
}

fn destination() -> DestinationName {
    DestinationName::new("slack")
}

fn form(text: &str) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("command", "/herald")
        .append_pair("text", text)
        .append_pair("channel_id", "C0123456789")
        .append_pair("user_id", "U0123456789")
        .append_pair("response_url", HOOK)
        .finish()
}

fn received(body: &str) -> Result<Received, Unread> {
    interacting("").received(&destination(), body.as_bytes())
}

fn asked(text: &str) -> Asked {
    match received(&form(text)) {
        Ok(Received::Asked { asked, replying }) => {
            assert_eq!(replying.url.expose(), HOOK);
            *asked
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_slash_command_is_read_with_its_project_and_share() {
    assert_eq!(
        asked("latest herald share"),
        Asked {
            destination: destination(),
            place: Place::new("C0123456789"),
            user: "U0123456789".to_owned(),
            command: Command::Latest,
            project: Some("herald".to_owned()),
            share: true,
            toggles: false,
            member: None,
        }
    );
    let plain = asked("Subscribe");
    assert_eq!(
        (plain.command, plain.project, plain.share),
        (Command::Subscribe, None, false)
    );
    let shared = asked("changelog SHARE");
    assert_eq!((shared.project, shared.share), (None, true));
    let named = asked("unsubscribe gate");
    assert_eq!(
        (named.project.as_deref(), named.share),
        (Some("gate"), false)
    );
}

#[test]
fn a_command_herald_cannot_read_is_answered_with_its_usage() {
    for text in ["", "deploy", "latest a b", "latest a b share"] {
        assert_eq!(received(&form(text)), Ok(Received::Usage), "{text}");
    }
    assert_eq!(
        serde_json::from_str::<Value>(
            &interacting(r#", "commands": ["latest", "changelog"]"#).usage()
        )
        .unwrap(),
        json!({
            "response_type": "ephemeral",
            "text": "Give /herald latest | changelog with a project, and share to show the answer to the channel.",
        })
    );
}

#[test]
fn another_slash_command_is_ignored_and_a_form_without_one_unread() {
    let other = form("latest").replace("%2Fherald", "%2Fdeploy");
    assert_eq!(received(&other), Ok(Received::Ignored));
    assert_eq!(received("text=latest"), Err(Unread::NotACommand));
}

#[test]
fn slack_checking_the_endpoint_s_certificate_is_answered_and_nothing_more() {
    assert_eq!(received("ssl_check=1&token=t"), Ok(Received::Ignored));
    assert_eq!(received("ssl_check=0"), Err(Unread::NotACommand));
}

#[test]
fn a_command_missing_its_place_or_answered_elsewhere_than_slack_is_unread() {
    for (field, value) in [
        ("channel_id", "C0123456789"),
        ("user_id", "U0123456789"),
        (
            "response_url",
            "https%3A%2F%2Fhooks.slack.com%2Fcommands%2FT1%2F2%2Fabc",
        ),
    ] {
        let missing = form("latest").replace(&format!("&{field}={value}"), "");
        assert_eq!(received(&missing), Err(Unread::Unplaced), "{field}");
        let empty = form("latest").replace(&format!("&{field}={value}"), &format!("&{field}="));
        assert_eq!(received(&empty), Err(Unread::Unplaced), "{field}");
    }
    let elsewhere = form("latest").replace("hooks.slack.com", "evil.example");
    assert_eq!(received(&elsewhere), Err(Unread::Unplaced));
    let lookalike = form("latest").replace("hooks.slack.com%2F", "hooks.slack.com.evil.example%2F");
    assert_eq!(received(&lookalike), Err(Unread::Unplaced));
}

fn payload(value: &Value) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("payload", &value.to_string())
        .finish()
}

fn pressed(action: &str) -> Value {
    json!({
        "type": "block_actions",
        "user": { "id": "U0123456789" },
        "channel": { "id": "C0123456789" },
        "response_url": HOOK,
        "actions": [{ "action_id": action, "value": "herald" }],
    })
}

#[test]
fn a_subscribe_button_press_toggles_and_other_presses_are_ignored() {
    let Ok(Received::Asked { asked, .. }) = received(&payload(&pressed(SUBSCRIBE))) else {
        panic!("a press is asked");
    };
    assert_eq!(asked.command, Command::Subscribe);
    assert_eq!(asked.project.as_deref(), Some("herald"));
    assert!(asked.toggles);
    assert_eq!(received(&payload(&pressed("other"))), Ok(Received::Ignored));
    let mut submission = pressed(SUBSCRIBE);
    submission["type"] = json!("view_submission");
    assert_eq!(received(&payload(&submission)), Ok(Received::Ignored));
    let mut actionless = pressed(SUBSCRIBE);
    actionless.as_object_mut().unwrap().remove("actions");
    assert_eq!(received(&payload(&actionless)), Ok(Received::Ignored));
    let mut unplaced = pressed(SUBSCRIBE);
    unplaced.as_object_mut().unwrap().remove("channel");
    assert_eq!(received(&payload(&unplaced)), Err(Unread::Unplaced));
    assert_eq!(received("payload=not%20json"), Err(Unread::NotACommand));
}

#[test]
fn an_answer_is_one_message_of_sections_seen_by_the_asker_unless_public() {
    let replying = Replying {
        url: Secret::new("the command's response URL", HOOK),
    };
    let parts = ["*one*".to_owned(), "two".to_owned()];
    let answer = interacting("").answered(&replying, &parts, false);
    assert_eq!(answer.method, Method::Post);
    assert_eq!(answer.url.expose(), HOOK);
    assert!(!answer.url.redacted().contains("abc"));
    assert_eq!(
        serde_json::from_slice::<Value>(&answer.body).unwrap(),
        json!({
            "response_type": "ephemeral",
            "replace_original": false,
            "text": "*one*",
            "blocks": [
                { "type": "section", "text": { "type": "mrkdwn", "text": "*one*" } },
                { "type": "section", "text": { "type": "mrkdwn", "text": "two" } },
            ],
        })
    );
    let public = interacting("").answered(&replying, &[], true);
    let body = serde_json::from_slice::<Value>(&public.body).unwrap();
    assert_eq!(body["response_type"], "in_channel");
    assert_eq!(body["text"], "");
    let many: Vec<String> = (0..60).map(|n| n.to_string()).collect();
    let capped = interacting("").answered(&replying, &many, false);
    assert_eq!(
        serde_json::from_slice::<Value>(&capped.body).unwrap()["blocks"]
            .as_array()
            .map(Vec::len),
        Some(50)
    );
}

#[test]
fn the_subscribe_block_holds_one_button_valued_by_its_project() {
    assert_eq!(
        subscribe_block("herald"),
        json!({
            "type": "actions",
            "elements": [{
                "type": "button",
                "action_id": "herald_subscribe",
                "value": "herald",
                "text": { "type": "plain_text", "text": "Subscribe" },
            }],
        })
    );
}

#[test]
fn slack_taking_an_answer_or_not_is_read_from_its_status() {
    let answer = |status, body: &str| Response {
        status,
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
    };
    assert_eq!(took(&answer(200, "ok")), Ok(()));
    assert_eq!(
        took(&answer(404, "expired_url")),
        Err("Slack answered 404: expired_url".to_owned())
    );
    assert_eq!(took(&answer(500, "")), Err("Slack answered 500".to_owned()));
    let long = "x".repeat(1000);
    assert_eq!(
        took(&answer(502, &long)),
        Err(format!("Slack answered 502: {}", "x".repeat(200)))
    );
}

#[test]
fn the_destination_says_where_it_reaches_and_how_it_answers() {
    let plain = interacting(r#", "replies": "public""#);
    assert_eq!(plain.api(), "https://slack.com/api");
    assert_eq!(plain.hooks(), "https://hooks.slack.com");
    assert_eq!(plain.via(), Via::Http);
    assert_eq!(
        plain.signing().map(|secret| secret.secret.as_str()),
        Some("HERALD_SLACK_SIGNING")
    );
    assert_eq!(plain.app(), None);
    assert_eq!(plain.answering().replies, Replies::Public);
    let socket = interacting(r#", "via": "socket", "app": {"secret": "HERALD_SLACK_APP"}"#);
    assert_eq!(socket.via(), Via::Socket);
    assert_eq!(
        socket.app().map(|secret| secret.secret.as_str()),
        Some("HERALD_SLACK_APP")
    );
    let recorded = Interacting::new(interactions(""), Some("http://127.0.0.1:9/"));
    assert_eq!(recorded.api(), "http://127.0.0.1:9");
    assert_eq!(recorded.hooks(), "http://127.0.0.1:9");
    assert_eq!(
        Unread::NotACommand.to_string(),
        "the body is neither a Slack command nor a button press"
    );
    assert_eq!(
        Unread::Unplaced.to_string(),
        "the request names no channel, member or Slack response URL of its own"
    );
}

#[test]
fn the_manifest_points_the_command_and_buttons_at_herald_or_opens_socket_mode() {
    let posted: Value = serde_json::from_str(
        &interacting(r#", "commands": ["latest"]"#)
            .manifest("slack", Some("https://herald.example/")),
    )
    .unwrap();
    let command = &posted["features"]["slash_commands"][0];
    assert_eq!(command["command"], "/herald");
    assert_eq!(command["url"], "https://herald.example/interactions/slack");
    assert_eq!(command["usage_hint"], "latest [project] [share]");
    assert_eq!(
        posted["settings"]["interactivity"],
        json!({ "is_enabled": true, "request_url": "https://herald.example/interactions/slack" })
    );
    assert_eq!(posted["settings"]["socket_mode_enabled"], false);
    assert_eq!(posted["oauth_config"]["scopes"]["bot"][2], "commands");
    let socket: Value = serde_json::from_str(&interacting("").manifest("slack", None)).unwrap();
    assert_eq!(socket["settings"]["socket_mode_enabled"], true);
    assert_eq!(
        socket["settings"]["interactivity"],
        json!({ "is_enabled": true })
    );
    assert_eq!(socket["features"]["slash_commands"][0].get("url"), None);
}
