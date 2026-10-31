use herald_ports::{Method, Secret, Text};
use serde_json::json;

use super::*;

fn message(content: &str) -> Request {
    Request::new(
        Method::Post,
        Text::plain("https://discord.com/api/v10/channels/1/messages"),
    )
    .header(
        "Authorization",
        Text::plain("Bot ").then_secret(Secret::new("TOKEN", "hunter2")),
    )
    .body(json!({ "content": content }).to_string())
}

fn fixture() -> Fixture {
    Fixture::from_json(
        r#"{"exchanges":[{
            "request":{"method":"POST","url":"https://discord.com/api/v10/channels/1/messages",
                "headers":{"Authorization":"Bot «secret:TOKEN»"},"body":{"content":"hi"}},
            "response":{"status":200,"headers":{"Content-Type":"application/json"},"body":{"id":"9"}}
        }]}"#,
    )
    .unwrap()
}

#[test]
fn a_request_is_recorded_with_its_secrets_redacted_and_its_json_as_json() {
    assert_eq!(
        Recorded::of(&message("hi")),
        Recorded {
            method: "POST".to_owned(),
            url: "https://discord.com/api/v10/channels/1/messages".to_owned(),
            headers: BTreeMap::from([(
                "Authorization".to_owned(),
                "Bot «secret:TOKEN»".to_owned()
            )]),
            body: Some(json!({ "content": "hi" })),
        }
    );
    let text = Request::new(Method::Get, Text::plain("https://example.org")).body("plain words");
    assert_eq!(Recorded::of(&text).body, Some(json!("plain words")));
    assert_eq!(
        Recorded::of(&Request::new(
            Method::Get,
            Text::plain("https://example.org")
        ))
        .body,
        None
    );
}

#[test]
fn a_replay_answers_the_request_it_holds_next() {
    let replay = Replay::of(fixture());
    let response = pollster::block_on(replay.send(message("hi"))).unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.body, br#"{"id":"9"}"#);
    assert_eq!(replay.finish(), Ok(()));
}

#[test]
fn a_replay_refuses_a_request_it_does_not_hold_and_says_so() {
    let replay = Replay::of(fixture());
    let refused = pollster::block_on(replay.send(message("bye")));
    assert_eq!(
        refused,
        Err(HttpError::Refused {
            url: "https://discord.com/api/v10/channels/1/messages".to_owned()
        })
    );
    let problems = replay.finish().unwrap_err();
    assert!(problems.contains(r#""content":"bye""#), "{problems}");
    assert!(
        problems.contains("never asked: POST https://discord.com/api/v10/channels/1/messages"),
        "{problems}"
    );
}

#[test]
fn a_replay_past_its_fixture_refuses_everything() {
    let replay = Replay::of(Fixture::default());
    assert!(pollster::block_on(replay.send(message("hi"))).is_err());
    assert!(
        replay
            .finish()
            .unwrap_err()
            .starts_with("expected nothing, got ")
    );
}

#[test]
fn a_recorder_records_what_it_sends_scrubbed() {
    let recorder = Recorder::new(
        Replay::of(fixture()),
        vec![("9".to_owned(), "1000".to_owned())],
    );
    pollster::block_on(recorder.send(message("hi"))).unwrap();
    let made = recorder.fixture().unwrap();
    assert_eq!(made.exchanges.len(), 1);
    assert_eq!(made.exchanges[0].request, fixture().exchanges[0].request);
    assert_eq!(
        made.exchanges[0].response.body,
        Some(json!({ "id": "1000" }))
    );
}

#[test]
fn a_recorder_scrubs_urls_and_nested_bodies() {
    let request = Request::new(
        Method::Put,
        Text::plain("https://matrix.example/rooms/!real:example/send"),
    )
    .body(
        json!({ "list": ["!real:example", 1], "nested": { "room": "!real:example" } }).to_string(),
    );
    let recorded = Recorded::of(&request)
        .scrubbed(&[("!real:example".to_owned(), "!room:example".to_owned())]);
    assert_eq!(
        recorded.url,
        "https://matrix.example/rooms/!room:example/send"
    );
    assert_eq!(
        recorded.body,
        Some(json!({ "list": ["!room:example", 1], "nested": { "room": "!room:example" } }))
    );
}

#[test]
fn a_failed_send_records_nothing() {
    let recorder = Recorder::new(Replay::of(Fixture::default()), vec![]);
    assert!(pollster::block_on(recorder.send(message("hi"))).is_err());
    assert_eq!(recorder.fixture(), Ok(Fixture::default()));
}

#[test]
fn an_answer_gives_back_its_body_as_it_was_recorded() {
    let answer = |body| Answer {
        status: 204,
        headers: BTreeMap::new(),
        body,
    };
    assert_eq!(answer(None).response().body, b"");
    assert_eq!(answer(Some(json!("text"))).response().body, b"text");
    assert_eq!(answer(Some(json!([1]))).response().body, b"[1]");
}

#[test]
fn a_fixture_round_trips_through_its_json() {
    let json = fixture().to_json();
    assert!(json.ends_with("}\n"));
    assert_eq!(Fixture::from_json(&json).unwrap(), fixture());
}

fn echo(body: &str) -> Fixture {
    Fixture {
        exchanges: vec![Exchange {
            request: Recorded::of(&message("hi")),
            response: Answer {
                status: 200,
                headers: BTreeMap::new(),
                body: Some(json!({ "echo": body })),
            },
        }],
    }
}

#[test]
fn a_secret_sent_through_the_recorder_never_reaches_the_written_file() {
    let recorder = Recorder::new(Replay::of(fixture()), vec![]);
    pollster::block_on(recorder.send(message("hi"))).unwrap();
    let written = recorder.fixture().unwrap().to_json();
    assert!(!written.contains("hunter2"), "{written}");
    assert!(written.contains("«secret:TOKEN»"));
}

#[test]
fn a_recording_that_echoes_a_secret_it_was_handed_is_refused() {
    let recorder = Recorder::new(Replay::of(echo("hunter2")), vec![]);
    pollster::block_on(recorder.send(message("hi"))).unwrap();
    assert_eq!(
        recorder.fixture(),
        Err("exchange 0 holds a secret it was handed".to_owned())
    );
}

#[test]
fn a_fixture_holding_anything_shaped_like_a_credential_is_refused() {
    let token = ["xox", "b-", "0123456789"].concat();
    assert_eq!(
        echo(&token).audit(&[]),
        Err("exchange 0 holds a Slack bot token".to_owned())
    );
    let json = echo(&token).to_json();
    assert_eq!(
        Fixture::from_json(&json),
        Err("exchange 0 holds a Slack bot token".to_owned())
    );
    let mut clear = fixture();
    clear.exchanges[0]
        .request
        .headers
        .insert("Authorization".to_owned(), "Bot plain".to_owned());
    assert_eq!(
        clear.audit(&[]),
        Err("exchange 0 has a Authorization header in the clear".to_owned())
    );
    assert!(Fixture::from_json("not json").is_err());
}

#[test]
fn a_plain_secret_the_recorder_was_never_told_of_is_still_found_by_its_shape() {
    let token = ["gh", "p_", "0123456789abcdef"].concat();
    let leaky = Request::new(
        Method::Post,
        Text::plain(format!("https://api.github.com/x?token={token}")),
    );
    let recorder = Recorder::new(Replay::of(Fixture::default()), vec![]);
    let _ = pollster::block_on(recorder.send(leaky));
    assert_eq!(recorder.fixture(), Ok(Fixture::default()));
    let mut leaked = fixture();
    leaked.exchanges[0].request.url = format!("https://api.github.com/x?token={token}");
    assert_eq!(leaked.audit(&[]),
        Err("exchange 0 holds a GitHub token".to_owned())
    );
    assert_eq!(fixture().audit(&[String::new()]), Ok(()));
}
