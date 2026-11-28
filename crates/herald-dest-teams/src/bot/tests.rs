use herald_core::status;

use super::*;
use crate::cloud::Cloud;

fn password() -> Secret {
    Secret::new("HERALD_TEAMS_APP_PASSWORD", ["p&", "1 ="].concat())
}

fn bot(threads: Option<bool>) -> Bot {
    Bot::new(
        "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9",
        password(),
        Cloud::Public.hosts(),
        &RouteOptions {
            conversation: Some("19:abc@thread.tacv2".to_owned()),
            threads,
        },
    )
}

fn answer(status: u16, body: &str) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
    }
}

#[test]
fn a_sign_in_asks_its_tenant_for_its_scope_s_default_with_its_body_sealed() {
    let request = bot(None).signing_in().unwrap();
    assert_eq!(request.method, Method::Post);
    assert_eq!(
        request.url.redacted(),
        "https://login.microsoftonline.com/botframework.com/oauth2/v2.0/token"
    );
    assert!(request.sealed);
    assert_eq!(request.most, TOKEN_ANSWER);
    assert_eq!(
        String::from_utf8(request.body).unwrap(),
        [
            "grant_type=client_credentials",
            "&client_id=0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9",
            "&client_secret=p%261%20%3D",
            "&scope=https%3A%2F%2Fapi.botframework.com%2F.default",
        ]
        .concat()
    );
    let unnamed = Bot {
        tenant: "..".to_owned(),
        ..bot(None)
    };
    assert_eq!(
        unnamed.signing_in().unwrap_err(),
        Declined::Unnamed("the bot's tenant")
    );
}

#[test]
fn a_refused_sign_in_names_its_error_and_first_code_and_never_its_description() {
    for (body, said) in [
        (
            r#"{"error":"invalid_client","error_description":"AADSTS7000215: secret p1","error_codes":[7000215,9]}"#,
            "invalid_client (AADSTS7000215)",
        ),
        (r#"{"error":"invalid_client"}"#, "invalid_client"),
        (r#"{"error_codes":[700016]}"#, "AADSTS700016"),
        (r#"{"error":"Not A Code p1"}"#, ""),
        (&format!(r#"{{"error":"{}"}}"#, "a".repeat(65)), ""),
        (
            &format!(r#"{{"error":"{}"}}"#, "a".repeat(64)),
            &"a".repeat(64),
        ),
        ("not json p1", ""),
    ] {
        assert_eq!(refused(&answer(401, body)), said, "{body}");
    }
}

#[test]
fn a_token_is_sent_only_as_printable_ascii_with_no_space() {
    assert!(sendable("eyJ0.abc-_~+/="));
    for token in ["", "a b", "a\r\nb", "é", "a\tb"] {
        assert!(!sendable(token), "{token:?}");
    }
}

#[test]
fn a_held_token_is_sent_until_its_time_and_never_after() {
    let held = Held::default();
    let now = Timestamp::UNIX_EPOCH;
    assert_eq!(held.current(now), None);
    let until = now.checked_add(SignedDuration::from_secs(10)).unwrap();
    held.keep(Secret::new(TOKEN, "t1"), until);
    let before = until.checked_sub(SignedDuration::from_secs(1)).unwrap();
    assert_eq!(held.current(before).unwrap().expose(), "t1");
    assert_eq!(held.current(until), None);
}

#[test]
fn an_answer_names_its_message_waits_on_412_and_names_a_refusal() {
    let waits = Waits::DEFAULT;
    assert_eq!(
        read(&answer(201, r#"{"id":"1:9"}"#), waits),
        Sent::Landed(Facts {
            message: Some("1:9".to_owned()),
            ..Facts::default()
        })
    );
    for body in ["", r#"{"id":""}"#, r#"{"id":7}"#, "{}"] {
        assert_eq!(
            read(&answer(200, body), waits),
            Sent::Landed(Facts::default()),
            "{body}"
        );
    }
    assert_eq!(
        read(&answer(status::PRECONDITION_FAILED, ""), waits),
        Sent::RetryAfter(waits.outage)
    );
    let mut named = answer(status::PRECONDITION_FAILED, "");
    named
        .headers
        .push(("Retry-After".to_owned(), "9".to_owned()));
    assert_eq!(
        read(&named, waits),
        Sent::RetryAfter(SignedDuration::from_secs(9))
    );
    assert_eq!(
        read(
            &answer(
                413,
                r#"{"error":{"code":"MessageSizeTooBig","message":"too big"}}"#
            ),
            waits
        ),
        Sent::Refused("Microsoft Teams refused it with 413: MessageSizeTooBig: too big".to_owned())
    );
}

#[test]
fn only_the_bot_framework_s_own_error_envelope_declines() {
    assert!(declines(&answer(
        403,
        r#"{"error":{"code":"BotNotInConversationRoster","message":"m"}}"#
    )));
    for body in [
        r#"{"error":{"code":403}}"#,
        r#"{"error":"forbidden"}"#,
        r#"{"errorCode":209,"message":"m"}"#,
        "<html></html>",
    ] {
        assert!(!declines(&answer(403, body)), "{body}");
    }
}

#[test]
fn it_reaches_its_sign_in_service_and_service_url_and_shows_neither_secret_nor_token() {
    let bot = bot(Some(true));
    assert_eq!(
        bot.bases(),
        [
            "https://login.microsoftonline.com".to_owned(),
            "https://smba.trafficmanager.net".to_owned()
        ]
    );
    assert!(bot.threads());
    let shown = format!("{bot:?}");
    assert!(
        shown.contains("«secret:HERALD_TEAMS_APP_PASSWORD»") && !shown.contains("p&"),
        "{shown}"
    );
    assert_eq!(bot, bot.clone());
    assert_ne!(
        bot,
        Bot {
            threads: false,
            ..bot.clone()
        }
    );
}

#[test]
fn a_bot_whose_tenant_names_nothing_asks_nobody_and_sends_nothing() {
    let unnamed = Bot {
        tenant: ".".to_owned(),
        ..bot(None)
    };
    let replay = herald_testing::Replay::of(herald_testing::Fixture::default());
    let probes = pollster::block_on(unnamed.probe(&replay));
    let unnamed_said = Declined::Unnamed("the bot's tenant").to_string();
    assert_eq!(probes, [Probe::Credential(unnamed_said.clone())]);
    let announcement = herald_testing::sample::announcement();
    let entry = Entry::new(announcement.key);
    assert_eq!(
        pollster::block_on(unnamed.send(
            &replay,
            (&Step::Post, &announcement, &entry),
            Timestamp::UNIX_EPOCH,
            Waits::DEFAULT
        ))
        .unwrap(),
        Sent::Refused(unnamed_said)
    );
    replay.finish().unwrap();
}
