//! What doctor says online of a route whose platform addresses its place
//! to anyone (ADR-0067).

use herald_config::Uses;
use herald_flows::{online, prepare};
use herald_testing::{FakeSecrets, FakeSigner};

use super::loaded;
use crate::harness::Script;

#[test]
fn online_a_private_route_to_a_room_anyone_may_read_is_a_mistake_and_another_route_a_note() {
    let configured = loaded(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.chat]
kind = "matrix"
homeserver = "https://matrix.example"
token = { secret = "T" }
[sources.gate]
kind = "github"
repository = "o/r"
[[routes]]
id = "team"
sources = ["gate"]
destination = "chat"
room = "!team:matrix.example"
template = "t"
private = true
[[routes]]
id = "hall"
sources = ["gate"]
destination = "chat"
room = "!hall:matrix.example"
template = "t"
[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
"#,
    );
    let given = FakeSecrets::default().with("T", "token");
    let prepared =
        pollster::block_on(prepare(&configured, &given, &FakeSigner, Uses::Polls)).unwrap();
    let readable = r#"{"history_visibility":"world_readable"}"#;
    let mut answers = Vec::new();
    for room in ["%21team%3Amatrix.example", "%21hall%3Amatrix.example"] {
        answers.push((
            "/account/whoami".to_owned(),
            r#"{"user_id":"@h:matrix.example"}"#,
        ));
        answers.push((
            format!("/rooms/{room}/state/m.room.power_levels/"),
            r#"{"users_default":100}"#,
        ));
        answers.push((
            format!("/rooms/{room}/state/m.room.join_rules/"),
            r#"{"join_rule":"invite"}"#,
        ));
        answers.push((
            format!("/rooms/{room}/state/m.room.history_visibility/"),
            readable,
        ));
    }
    let answers: Vec<(&str, u16, &str)> = answers
        .iter()
        .map(|(line, body)| (line.as_str(), 200, *body))
        .collect();
    let script = Script::answering(&answers);
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let findings = pollster::block_on(online(&connected, (&FakeSigner, crate::harness::NOON)));
    assert_eq!(script.seen().len(), 8, "{:?}", script.seen());
    assert_eq!(
        findings.mistakes,
        [
            "The route team says private = true, but anyone may read the history of !team:matrix.example, as its m.room.history_visibility is world_readable; a section only private routes are shown, such as a view's advisories, would be published there: remove private = true, or make the place private"
        ]
    );
    assert!(
        findings.notes.iter().any(|note| note
            == "The route hall: anyone may read the history of !hall:matrix.example, as its m.room.history_visibility is world_readable."),
        "{:?}",
        findings.notes
    );
}
