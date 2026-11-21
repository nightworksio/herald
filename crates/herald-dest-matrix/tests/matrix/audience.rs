//! Who may read a Matrix route's room, read right before a run's first
//! advisories section is sent there (ADR-0067).

use herald_dest_matrix::{Matrix, RouteOptions};
use herald_kinds::{Audience, Reach};
use herald_ports::{Response, Secret};
use herald_testing::ScriptedHttp;
use serde_json::json;

fn answer(body: &serde_json::Value) -> Response {
    Response {
        status: 200,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

#[test]
fn a_room_is_private_only_where_joining_is_closed_and_its_history_not_world_readable() {
    let matrix = Matrix::new(
        "https://matrix.example",
        Secret::new("HERALD_MATRIX_TOKEN", "test-token"),
        RouteOptions {
            room: "!abcdefg:matrix.example".to_owned(),
        },
    );
    let read = |join: &str, history: &str| {
        let http = ScriptedHttp::new(vec![
            answer(&json!({ "join_rule": join })),
            answer(&json!({ "history_visibility": history })),
        ]);
        let audience = pollster::block_on(matrix.audience(&http));
        (
            audience,
            http.asked()
                .into_iter()
                .map(|(url, _)| url)
                .collect::<Vec<_>>(),
        )
    };
    let (private, asked) = read("restricted", "joined");
    assert_eq!(private, Audience::Private);
    assert_eq!(
        asked,
        [
            "https://matrix.example/_matrix/client/v3/rooms/%21abcdefg%3Amatrix.example/state/m.room.join_rules/",
            "https://matrix.example/_matrix/client/v3/rooms/%21abcdefg%3Amatrix.example/state/m.room.history_visibility/",
        ]
    );
    for (join, history) in [
        ("public", "joined"),
        ("invite", "world_readable"),
        ("unheard", "shared"),
    ] {
        assert!(
            matches!(read(join, history).0, Audience::Public(_)),
            "{join} {history}"
        );
    }
    assert!(matrix.keeps_edits());
    let unasked = ScriptedHttp::new(Vec::new());
    assert!(matches!(
        pollster::block_on(matrix.audience(&unasked)),
        Audience::Unread(_)
    ));
}

#[test]
fn a_restricted_room_is_private_only_where_every_room_it_lets_in_takes_an_invite_or_a_knock() {
    let matrix = Matrix::new(
        "https://matrix.example",
        Secret::new("HERALD_MATRIX_TOKEN", "test-token"),
        RouteOptions {
            room: "!abcdefg:matrix.example".to_owned(),
        },
    );
    let member = |room: &str| json!({ "type": "m.room_membership", "room_id": room });
    let read = |allow: Vec<serde_json::Value>, theirs: Vec<serde_json::Value>| {
        let mut answers = vec![
            answer(&json!({ "join_rule": "knock_restricted", "allow": allow })),
            answer(&json!({ "history_visibility": "joined" })),
        ];
        answers.extend(theirs.iter().map(answer));
        let http = ScriptedHttp::new(answers);
        let audience = pollster::block_on(matrix.audience(&http));
        let asked: Vec<String> = http.asked().into_iter().map(|(url, _)| url).collect();
        (audience, asked)
    };
    let (private, asked) = read(
        vec![
            member("!space:matrix.example"),
            member("!team:matrix.example"),
        ],
        vec![
            json!({ "join_rule": "invite" }),
            json!({ "join_rule": "knock" }),
        ],
    );
    assert_eq!(private, Audience::Private);
    assert_eq!(
        asked[2..],
        [
            "https://matrix.example/_matrix/client/v3/rooms/%21space%3Amatrix.example/state/m.room.join_rules/",
            "https://matrix.example/_matrix/client/v3/rooms/%21team%3Amatrix.example/state/m.room.join_rules/",
        ]
    );
    for theirs in ["public", "restricted", "knock_restricted"] {
        let (unread, _) = read(
            vec![member("!space:matrix.example")],
            vec![json!({ "join_rule": theirs, "allow": [] })],
        );
        assert!(matches!(unread, Audience::Unread(_)), "{theirs} {unread:?}");
    }
    let (unasked, _) = read(vec![member("!space:matrix.example")], Vec::new());
    assert!(matches!(unasked, Audience::Unread(_)), "{unasked:?}");
    let (odd, asked) = read(
        vec![json!({ "type": "m.other", "room_id": "!x:y" })],
        Vec::new(),
    );
    assert!(matches!(odd, Audience::Unread(_)), "{odd:?}");
    assert_eq!(asked.len(), 2);
    let many: Vec<serde_json::Value> = (0..6)
        .map(|n| member(&format!("!r{n}:matrix.example")))
        .collect();
    let (past, asked) = read(many, Vec::new());
    assert_eq!(
        past,
        Audience::Unread(
            "!abcdefg:matrix.example lets in the members of 6 rooms, more than the 5 herald checks"
                .to_owned()
        )
    );
    assert_eq!(asked.len(), 2);
    let most: Vec<serde_json::Value> = (0..5)
        .map(|n| member(&format!("!r{n}:matrix.example")))
        .collect();
    let invited = vec![json!({ "join_rule": "invite" }); 5];
    let (all, asked) = read(most, invited);
    assert_eq!(all, Audience::Private);
    assert_eq!(asked.len(), 7);
    let (dotted, asked) = read(vec![member("..")], Vec::new());
    assert_eq!(
        dotted,
        Audience::Unread(
            "the Matrix id in the request's path is empty or a dot segment, which names nothing in a URL path, so this step is not taken"
                .to_owned()
        )
    );
    assert_eq!(asked.len(), 2);
}
