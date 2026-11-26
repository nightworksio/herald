//! A room that is no single path segment, `..` read from a config no check
//! passed, and an event id that is not one: refused before a request, at
//! the point each was refused before (ADR-0067).

use herald_core::{Declined, Delivery, Entry, Facts, Sent, Step};
use herald_dest_matrix::{Matrix, MatrixMarkers, RouteOptions};
use herald_kinds::{Probe, Reach, Recorded};
use herald_ports::{Markers, Response, Secret, USER_AGENT};
use herald_testing::sample::{announcement, marker};
use herald_testing::{Fixture, Replay, ScriptedHttp};
use serde_json::{Value, json};

const DOTTED: &str = "the Matrix id in the request's path is empty or a dot segment, which names nothing in a URL path, so this step is not taken";

fn matrix(room: &str) -> Matrix {
    Matrix::new(
        "https://matrix.example",
        Secret::new("HERALD_MATRIX_TOKEN", "test-token"),
        RouteOptions {
            room: room.to_owned(),
        },
    )
}

fn answer(body: &Value) -> Response {
    Response {
        status: 200,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

fn posted(event: &str) -> Entry {
    Entry::new(announcement().key).landed(
        Step::Post,
        Facts {
            message: Some(event.to_owned()),
            ..Facts::default()
        },
    )
}

/// A replay of `exchanges` alone, refusing any other request.
fn replay(exchanges: &Value) -> Replay {
    Replay::of(Fixture::from_json(&json!({ "exchanges": exchanges }).to_string()).unwrap())
}

/// The homeserver answering a read of `path` with `body`.
fn read_exchange(path: &str, body: &Value) -> Value {
    json!({
        "request": {
            "method": "GET",
            "url": format!("https://matrix.example/_matrix/client/v3{path}"),
            "headers": {
                "Authorization": "Bearer «secret:HERALD_MATRIX_TOKEN»",
                "User-Agent": USER_AGENT
            }
        },
        "response": { "status": 200, "body": body }
    })
}

/// The homeserver's answer naming herald's own account.
fn whoami_exchange() -> Value {
    read_exchange(
        "/account/whoami",
        &json!({ "user_id": "@herald:matrix.example" }),
    )
}

/// What `step` of `entry` sends through a replay asking nothing, and
/// whether it asked nothing.
fn sent(matrix: &Matrix, step: &Step, entry: &Entry) -> (Sent, Result<(), String>) {
    let http = replay(&json!([]));
    let sent = pollster::block_on(matrix.send(&http, step, &announcement(), entry)).unwrap();
    (sent, http.finish())
}

fn whoami() -> Response {
    answer(&json!({ "user_id": "@herald:matrix.example" }))
}

#[test]
fn a_dotted_room_is_pinned_unpinned_redacted_or_read_back_with_no_request() {
    let dotted = matrix("..");
    let refused = (Sent::Refused(DOTTED.to_owned()), Ok(()));
    assert_eq!(sent(&dotted, &Step::Pin, &posted("$post")), refused);
    assert_eq!(sent(&dotted, &Step::Unpin, &posted("$post")), refused);
    let pinned = posted("$post").landed(Step::Pin, Facts::default());
    assert_eq!(sent(&dotted, &Step::Delete(0), &pinned), refused);
    assert_eq!(
        dotted
            .request(&Step::Pin, &announcement(), &posted("$post"))
            .unwrap_err()
            .to_string(),
        DOTTED
    );
    let http = replay(&json!([whoami_exchange()]));
    let read = pollster::block_on(
        MatrixMarkers::new(&dotted, &http).entry(&announcement().key, &marker()),
    );
    assert_eq!(
        read.unwrap_err().to_string(),
        format!("the destination's posts could not be read: {DOTTED}")
    );
    assert_eq!(http.finish(), Ok(()));
    let checking = ScriptedHttp::new(vec![whoami()]);
    let probes = pollster::block_on(dotted.probe(&checking, &Delivery::default(), Recorded::Kept));
    assert_eq!(
        probes,
        [
            Probe::Taken,
            Probe::Route {
                key: Some("room"),
                why: DOTTED.to_owned(),
            },
        ]
    );
    assert_eq!(checking.asked().len(), 1);
}

#[test]
fn a_pin_named_by_a_dotted_event_is_never_read_back() {
    let room = matrix("!abcdefg:matrix.example");
    let mut unremembered = announcement();
    unremembered.replaces = None;
    let http = replay(&json!([
        read_exchange(
            "/rooms/%21abcdefg%3Amatrix.example/state/m.room.pinned_events",
            &json!({ "pinned": ["..", "$post"] })
        ),
        whoami_exchange(),
    ]));
    let sent = pollster::block_on(room.send(&http, &Step::Unpin, &unremembered, &posted("$post")))
        .unwrap();
    assert_eq!(sent, Sent::Refused(DOTTED.to_owned()));
    assert_eq!(http.finish(), Ok(()));
}

#[test]
fn an_event_id_is_a_dollar_and_at_least_one_more_character() {
    let room = matrix("!abcdefg:matrix.example");
    assert_eq!(
        room.request(&Step::Chunk(1), &announcement(), &posted("$")),
        Err(Declined::Malformed("Matrix event id"))
    );
    assert!(
        room.request(&Step::Chunk(1), &announcement(), &posted("$p"))
            .is_ok()
    );
}

#[test]
fn only_an_answer_naming_a_matrix_error_code_declines_a_step() {
    let room = matrix("!abcdefg:matrix.example");
    assert!(room.declines(&answer(&json!({ "errcode": "M_FORBIDDEN", "error": "no" }))));
    for undeclined in [json!({ "error": "no" }), json!({})] {
        assert!(!room.declines(&answer(&undeclined)), "{undeclined}");
    }
}
