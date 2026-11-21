//! Which Slack answers are its own refusal of a request, so a first send
//! answered with one left nothing behind (ADR-0067).

use herald_core::media::JSON;
use herald_dest_slack::{RouteOptions, Slack};
use herald_kinds::Reach;
use herald_ports::{Credential, Response, Secret};

fn answered(body: &str) -> Response {
    Response {
        status: 403,
        headers: vec![("Content-Type".to_owned(), JSON.to_owned())],
        body: body.as_bytes().to_vec(),
    }
}

#[test]
fn only_an_error_that_settles_the_request_is_a_refusal() {
    let slack = Slack::new(
        Credential::Bot(Secret::new("HERALD_SLACK_TOKEN", "test-token")),
        RouteOptions::default(),
        None,
    );
    assert!(slack.declines(&answered(r#"{"ok": false, "error": "not_in_channel"}"#)));
    for error in [
        "fatal_error",
        "internal_error",
        "service_unavailable",
        "request_timeout",
        "ratelimited",
    ] {
        let said = format!(r#"{{"ok": false, "error": "{error}"}}"#);
        assert!(!slack.declines(&answered(&said)), "{error}");
    }
    for said in [r#"{"ok": false}"#, r#"{"ok": true, "error": "x"}"#, "{}"] {
        assert!(!slack.declines(&answered(said)), "{said}");
    }
}
