//! Which client errors each kind of destination takes for its platform's
//! own refusal, so a first send answered with one left nothing behind,
//! never a page something in front of the platform answered with
//! (ADR-0067).

use herald_core::media::JSON;
use herald_ports::{Response, Secret};

use crate::addressed::loaded;

/// Every kind of destination herald ships, and the error envelope its
/// platform declines a request with, where it has one.
const ENVELOPES: [(&str, Option<&str>); 7] = [
    (
        "discord",
        Some(r#"{"code": 50013, "message": "Missing Permissions"}"#),
    ),
    ("mastodon", Some(r#"{"error": "Validation failed"}"#)),
    (
        "matrix",
        Some(r#"{"errcode": "M_FORBIDDEN", "error": "not in the room"}"#),
    ),
    ("slack", Some(r#"{"ok": false, "error": "not_in_channel"}"#)),
    ("activitypub", None),
    ("herald", None),
    ("herald-push", None),
];

/// A `403` answered as JSON with `body`.
fn forbidden(body: &str) -> Response {
    Response {
        status: 403,
        headers: vec![("Content-Type".to_owned(), JSON.to_owned())],
        body: body.as_bytes().to_vec(),
    }
}

#[test]
fn every_kind_of_destination_decides_which_answers_are_its_platform_s_own_refusal() {
    let listed: Vec<&str> = ENVELOPES.iter().map(|(kind, _)| *kind).collect();
    assert_eq!(crate::common::kinds().destination_kinds(), listed);
    for (kind, envelope) in ENVELOPES {
        let config = loaded(kind, "").unwrap();
        let route = &config.routes[0];
        let reached = config.destinations[&route.route.destination]
            .reach(&route.options, "", Secret::new("HERALD_PLACE", "unread"))
            .unwrap();
        assert_eq!(
            envelope.is_some_and(|envelope| reached.declines(&forbidden(envelope))),
            envelope.is_some(),
            "{kind}"
        );
        for other in [
            r#"{"detail": "blocked by the gateway"}"#,
            "<html><body>Request blocked</body></html>",
            "",
        ] {
            assert!(!reached.declines(&forbidden(other)), "{kind}: {other}");
        }
    }
}

#[test]
fn a_slack_error_that_may_follow_a_partial_effect_or_says_nothing_of_the_request_is_no_refusal() {
    let config = loaded("slack", "").unwrap();
    let route = &config.routes[0];
    let reached = config.destinations[&route.route.destination]
        .reach(&route.options, "", Secret::new("HERALD_PLACE", "unread"))
        .unwrap();
    for error in [
        "fatal_error",
        "internal_error",
        "service_unavailable",
        "request_timeout",
        "ratelimited",
    ] {
        let said = format!(r#"{{"ok": false, "error": "{error}"}}"#);
        assert!(!reached.declines(&forbidden(&said)), "{error}");
    }
    assert!(reached.declines(&forbidden(r#"{"ok": false, "error": "channel_not_found"}"#)));
}
