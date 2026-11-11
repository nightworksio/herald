//! What a homeserver says of a bot when doctor asks, against its fixtures.

#[cfg(test)]
mod probe {
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
            Fixture::from_json(include_str!("../fixtures/probe.reference.json")).unwrap();
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
        let both = [(200, None), (200, None)];
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
            probed(&answered(&[(200, None), (200, Some(muted))]), true),
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
            probed(&answered(&[(200, None), (200, Some(open))]), true),
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
}
