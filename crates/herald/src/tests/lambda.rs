use herald_native::RingSigner;
use herald_ports::{Algorithm, Secret, Signer as _};
use serde_json::{Value, json};

use super::world::*;
use crate::STOPPED;

const LAMBDA: [&str; 1] = ["lambda"];

fn hooked() -> String {
    CONFIG.replace(
        "repository = \"cli/cli\"",
        "repository = \"cli/cli\"\nwebhook = { secret = \"HERALD_WEBHOOK\" }",
    )
}

fn function(config: &str, handing: bool) -> Setup {
    let mut setup = setup(config);
    let mut vars = vec![
        ("AWS_LAMBDA_RUNTIME_API", "127.0.0.1:9001"),
        ("HERALD_WEBHOOK", "hook-secret"),
    ];
    if handing {
        vars.extend([
            ("AWS_LAMBDA_FUNCTION_NAME", "herald"),
            ("AWS_REGION", "eu-west-1"),
            ("AWS_ACCESS_KEY_ID", "AKID"),
            ("AWS_SECRET_ACCESS_KEY", "secret-key"),
            ("AWS_SESSION_TOKEN", "session"),
        ]);
    }
    for (name, value) in vars {
        setup.vars.insert(name.to_owned(), value.to_owned());
    }
    setup
}

fn webhook() -> Value {
    let signature = on_tokio(RingSigner.sign(
        Algorithm::HmacSha256,
        &Secret::new("HERALD_WEBHOOK", "hook-secret"),
        EVENT.as_bytes(),
    ))
    .unwrap();
    json!({
        "version": "2.0",
        "requestContext": { "http": { "method": "POST" } },
        "rawPath": "/hooks/cli",
        "rawQueryString": "",
        "headers": {
            "x-hub-signature-256": format!("sha256={}", herald_core::hex(&signature)),
            "x-github-event": "release",
            "x-github-delivery": "d-1",
        },
        "body": EVENT,
        "isBase64Encoded": false,
    })
}

fn invoked(world: &World, events: Vec<Value>) {
    world.state.lock().unwrap().invocations.extend(events);
}

fn given(world: &World) -> Vec<(String, Value)> {
    world.state.lock().unwrap().given.clone()
}

#[test]
fn a_webhook_is_answered_its_work_handed_to_the_function_and_announced_there_once() {
    let world = World::default();
    discord(&world, &[("/channels/100/messages", 200, r#"{"id":"1"}"#)]);
    invoked(&world, vec![webhook(), json!({ "source": "aws.events" })]);
    let run = herald(&function(&hooked(), true), &world, &LAMBDA);
    assert_eq!(run.code, STOPPED, "{}{}", run.out, run.err);
    let given = given(&world);
    let paths: Vec<&str> = given.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "request-1/response",
            "request-2/response",
            "request-3/response"
        ],
        "{}",
        run.out
    );
    assert_eq!(given[0].1["statusCode"], json!(202), "{}", run.out);
    assert_eq!(given[0].1["isBase64Encoded"], json!(false));
    assert!(
        run.out.contains("releases: announced (post)"),
        "{}",
        run.out
    );
    assert!(
        run.out
            .ends_with("the runtime API could not be reached: the host did not answer in time; the function stops\n"),
        "{}",
        run.out
    );
    let seen = world.state.lock().unwrap().seen.clone();
    assert_eq!(
        seen.iter()
            .filter(|line| line.ends_with("/channels/100/messages"))
            .count(),
        1
    );
}

#[test]
fn work_that_cannot_be_handed_on_is_done_before_the_answer() {
    for handing in [false, true] {
        let world = World::default();
        world.state.lock().unwrap().handing = Some(500);
        discord(&world, &[("/channels/100/messages", 200, r#"{"id":"1"}"#)]);
        invoked(&world, vec![webhook()]);
        let run = herald(&function(&hooked(), handing), &world, &LAMBDA);
        assert!(
            run.out.contains("releases: announced (post)"),
            "{}",
            run.out
        );
        assert_eq!(
            run.out.contains(
                "the work could not be handed on, as Lambda answered 500; it is done now"
            ),
            handing,
            "{}",
            run.out
        );
        assert_eq!(given(&world).len(), 1);
    }
}

#[test]
fn an_event_herald_does_not_take_is_answered_as_an_error() {
    let world = World::default();
    invoked(&world, vec![json!({ "detail": "anything" })]);
    let run = herald(&function(CONFIG, false), &world, &LAMBDA);
    let given = given(&world);
    assert_eq!(given[0].0, "request-1/error");
    assert_eq!(
        given[0].1["errorMessage"],
        json!("herald takes requests, its own work and scheduled events alone")
    );
    assert!(run.out.contains("herald takes requests"), "{}", run.out);
}

#[test]
fn a_function_without_its_runtime_api_does_not_start() {
    let world = World::default();
    let run = herald(&setup(CONFIG), &world, &LAMBDA);
    assert_eq!(run.code, STOPPED);
    assert_eq!(
        run.err,
        "AWS_LAMBDA_RUNTIME_API is not set; herald lambda runs as a Lambda function's runtime\n"
    );
}

#[test]
fn a_runtime_api_that_refuses_stops_the_function_and_a_lost_answer_is_said() {
    let world = World::default();
    world.state.lock().unwrap().runtime = Some(500);
    let run = herald(&function(CONFIG, false), &world, &LAMBDA);
    assert!(
        run.out
            .ends_with("the runtime API answered 500; the function stops\n"),
        "{}",
        run.out
    );
    let world = World::default();
    world.state.lock().unwrap().unheard = true;
    invoked(&world, vec![json!({ "source": "aws.scheduler" })]);
    let run = herald(&function(CONFIG, false), &world, &LAMBDA);
    assert!(
        run.out.contains(
            "the answer to request-1 could not be given: the host did not answer in time"
        ),
        "{}",
        run.out
    );
}

#[test]
fn work_whose_hand_off_times_out_is_done_now_and_an_unread_body_is_not_a_request() {
    let world = World::default();
    world
        .state
        .lock()
        .unwrap()
        .unanswered
        .push("/functions/herald/invocations".to_owned());
    discord(&world, &[("/channels/100/messages", 200, r#"{"id":"1"}"#)]);
    let mut unread = webhook();
    unread["isBase64Encoded"] = json!(true);
    unread["body"] = json!("not base64!");
    invoked(&world, vec![webhook(), unread]);
    let run = herald(&function(&hooked(), true), &world, &LAMBDA);
    assert!(
        run.out.contains(
            "the work could not be handed on: the host did not answer in time; it is done now"
        ),
        "{}",
        run.out
    );
    assert!(
        run.out.contains("releases: announced (post)"),
        "{}",
        run.out
    );
    assert_eq!(given(&world)[1].0, "request-2/error");
}

#[test]
fn a_body_that_is_not_text_is_answered_in_base64() {
    assert_eq!(
        crate::lambda::replied(200, "application/octet-stream", &[0xff, 0x00]),
        json!({
            "statusCode": 200,
            "headers": { "content-type": "application/octet-stream" },
            "body": "/wA=",
            "isBase64Encoded": true,
        })
    );
}
