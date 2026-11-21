//! What a homeserver says of a bot when doctor asks, against its fixtures.

use herald_core::{Delivery, Pinning};
use herald_dest_matrix::{Matrix, RouteOptions};
use herald_kinds::{Probe, Reach};
use herald_ports::Secret;
use herald_testing::{Fixture, Replay};
use serde_json::{Value, json};

const ROOM: &str = "!abcdefg:matrix.example";

fn matrix() -> Matrix {
    Matrix::new(
        "https://matrix.example",
        Secret::new("HERALD_MATRIX_TOKEN", "test-token"),
        RouteOptions {
            room: ROOM.to_owned(),
        },
    )
}

fn answered(answers: &[(u16, Option<Value>)]) -> Replay {
    let mut fixture =
        Fixture::from_json(include_str!("../../fixtures/probe.reference.json")).unwrap();
    fixture.exchanges.truncate(answers.len());
    for (exchange, (status, body)) in fixture.exchanges.iter_mut().zip(answers) {
        exchange.response.status = *status;
        if let Some(body) = body {
            exchange.response.body = Some(body.clone());
        }
    }
    Replay::of(fixture)
}

fn probed(http: &Replay, pinning: bool) -> Vec<Probe> {
    let delivery = Delivery {
        pin: pinning.then_some(Pinning::Markers),
        ..Delivery::default()
    };
    let probes = pollster::block_on(matrix().probe(http, &delivery));
    http.finish().unwrap();
    probes
}

#[test]
fn a_bot_is_held_to_the_power_each_event_it_sends_needs() {
    let both = [(200, None), (200, None), (200, None), (200, None)];
    assert_eq!(probed(&answered(&both), false), [Probe::Taken]);
    assert_eq!(
        probed(&answered(&both), true),
        [
            Probe::Taken,
            Probe::Route {
                key: Some("room"),
                why: format!(
                    "@herald:matrix.example needs power level 50 in {ROOM} to send m.room.pinned_events; raise the bot's power level there"
                ),
            },
        ]
    );
    let muted = json!({ "events_default": 20, "users_default": 10 });
    assert_eq!(
        probed(
            &answered(&[(200, None), (200, Some(muted)), (200, None), (200, None)]),
            true
        ),
        [
            Probe::Taken,
            Probe::Route {
                key: Some("room"),
                why: format!(
                    "@herald:matrix.example needs power level 20 in {ROOM} to send m.room.message; raise the bot's power level there"
                ),
            },
            Probe::Route {
                key: Some("room"),
                why: format!(
                    "@herald:matrix.example needs power level 50 in {ROOM} to send m.room.pinned_events; raise the bot's power level there"
                ),
            },
        ]
    );
    let open = json!({ "users_default": 50 });
    assert_eq!(
        probed(
            &answered(&[(200, None), (200, Some(open)), (200, None), (200, None)]),
            true
        ),
        [Probe::Taken]
    );
}

#[test]
fn a_bot_outside_its_room_or_refused_is_told_so() {
    assert_eq!(
        probed(&answered(&[(200, None), (403, None)]), false),
        [
            Probe::Taken,
            Probe::Route {
                key: Some("room"),
                why: format!(
                    "@herald:matrix.example cannot read the power levels of {ROOM}, as the homeserver answered 403; invite the bot to the room and have it join"
                ),
            },
        ]
    );
    assert_eq!(
        probed(
            &answered(&[(200, None), (200, Some(json!("not json")))]),
            false
        ),
        [
            Probe::Taken,
            Probe::Said(format!(
                "the homeserver's power levels for {ROOM} could not be read, so the bot's power there is not checked"
            )),
        ]
    );
    assert_eq!(
        probed(&answered(&[(401, None)]), false),
        [Probe::Credential(
            "The homeserver refuses its credential, answering 401".to_owned()
        )]
    );
    assert_eq!(
        probed(&answered(&[(200, Some(json!({})))]), false),
        [Probe::Taken]
    );
}

#[test]
fn a_room_anyone_may_read_or_join_is_said_to_be_addressed_to_anyone() {
    let open = json!({ "users_default": 50 });
    let invite = json!({ "join_rule": "invite" });
    let probe = |join: (u16, Option<Value>), history: (u16, Option<Value>)| {
        let mut answers = vec![(200, None), (200, Some(open.clone())), join.clone()];
        let read = join
            .1
            .as_ref()
            .is_none_or(|body| body.get("join_rule").is_some());
        if join.0 == 200 && read {
            answers.push(history);
        }
        probed(&answered(&answers), false)
    };
    let readable = json!({ "history_visibility": "world_readable" });
    assert_eq!(
        probe((200, Some(invite.clone())), (200, Some(readable))),
        [
            Probe::Taken,
            Probe::Public(format!(
                "anyone may read the history of {ROOM}, as its m.room.history_visibility is world_readable"
            )),
        ]
    );
    let shared = json!({ "history_visibility": "shared" });
    assert_eq!(
        probe(
            (200, Some(json!({ "join_rule": "public" }))),
            (200, Some(shared.clone()))
        ),
        [
            Probe::Taken,
            Probe::Public(format!(
                "any Matrix user may join {ROOM}, as its m.room.join_rules is public"
            )),
        ]
    );
    for rule in ["invite", "knock", "restricted", "knock_restricted"] {
        let closed = json!({ "join_rule": rule });
        assert_eq!(
            probe((200, Some(closed)), (200, Some(shared.clone()))),
            [Probe::Taken],
            "{rule}"
        );
    }
    for (join, history) in [
        ((404, None), (200, Some(shared.clone()))),
        ((200, Some(invite.clone())), (404, None)),
        ((200, Some(json!({}))), (200, Some(shared))),
    ] {
        let probes = probe(join, history);
        assert!(
            matches!(&probes[..], [Probe::Taken, Probe::Said(said)] if said.starts_with("herald withholds advisories sections from this room: ")),
            "{probes:?}"
        );
    }
}
