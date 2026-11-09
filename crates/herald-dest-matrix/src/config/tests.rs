use herald_core::{ReadBack, Waits};
use herald_kinds::{Connect, Effect};
use herald_testing::FakeSecrets;
use serde_json::{Value, json};

use super::*;
use crate::interactions::SYNC_WAIT;

const ROOM: &str = "!room:matrix.example";

fn options(interactions: Option<Value>) -> DestinationOptions {
    let mut value = json!({
        "homeserver": "https://matrix.example/",
        "token": { "secret": "HERALD_MATRIX_TOKEN" },
    });
    if let Some(interactions) = interactions {
        value["interactions"] = interactions;
    }
    serde_json::from_value(value).unwrap()
}

fn appservice() -> Value {
    json!({
        "homeserver_token": { "secret": "HERALD_MATRIX_HS" },
        "appservice_token": { "secret": "HERALD_MATRIX_AS" },
    })
}

fn syncing() -> Value {
    json!({ "via": "sync" })
}

fn route(room: &str) -> RouteOptions {
    serde_json::from_value(json!({ "room": room })).unwrap()
}

fn nothing(_: &str) -> Result<String, String> {
    Err("no file".to_owned())
}

fn token() -> Secret {
    Secret::new("HERALD_MATRIX_TOKEN", "bot-token")
}

fn secrets() -> FakeSecrets {
    FakeSecrets::default()
        .with("HERALD_MATRIX_HS", "hs-token")
        .with("HERALD_MATRIX_AS", "as-token")
}

fn interacted(options: &DestinationOptions) -> Interacting {
    let interacted = pollster::block_on(options.interacts(&token(), &secrets()))
        .unwrap()
        .unwrap();
    interacted.of::<Interacting>().unwrap().clone()
}

fn destination() -> DestinationName {
    DestinationName::new("matrix")
}

fn message(body: &str) -> Value {
    json!({
        "type": "m.room.message",
        "room_id": ROOM,
        "event_id": "$command",
        "sender": "@ann:matrix.example",
        "content": { "msgtype": "m.text", "body": body },
    })
}

fn read(interacting: &Interacting, method: &str, path: &[&str], body: &Value) -> Read {
    let body = body.to_string();
    interacting.read(
        &destination(),
        Post {
            method,
            path,
            body: body.as_bytes(),
        },
    )
}

fn response(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

#[test]
fn a_destination_reads_as_a_matrix_one_through_its_bot() {
    let mut written = options(Some(appservice()));
    written.symbols = serde_json::from_value(json!({ "bullet": "*" })).unwrap();
    written.waits = serde_json::from_value(json!({ "outage": "7s" })).unwrap();
    assert_eq!(written.kind(), "matrix");
    assert_eq!(written.symbols_written(), written.symbols);
    assert_ne!(written.symbols_written(), Some(Symbols::default()));
    assert_eq!(written.waits_written(), written.waits);
    assert_ne!(written.waits_written(), Some(WaitOptions::default()));
    assert_eq!(
        written.credential().map(|named| named.secret.as_str()),
        Some("HERALD_MATRIX_TOKEN")
    );
    assert_eq!(
        written
            .secret_refs()
            .iter()
            .map(|named| named.secret.as_str())
            .collect::<Vec<_>>(),
        [
            "HERALD_MATRIX_TOKEN",
            "HERALD_MATRIX_HS",
            "HERALD_MATRIX_AS"
        ]
    );
    assert_eq!(
        written.answering(),
        written.interactions.as_ref().map(Interactions::answering)
    );
    assert_eq!(
        written.carried(),
        Some(Carried {
            posted: true,
            what: "transactions the homeserver pushes",
            session: "a sync with via = \"sync\"",
        })
    );
    assert_eq!(
        options(Some(syncing()))
            .carried()
            .map(|carried| carried.posted),
        Some(false)
    );
    assert_eq!(options(None).answering(), None);
    assert_eq!(options(None).carried(), None);
}

#[test]
fn a_route_reaches_and_profiles_its_room_through_the_bot() {
    let room = route(ROOM);
    assert_eq!(room.place(), Some(ROOM));
    let placed = Placed::new(room.clone());
    let destination = options(None);
    assert_eq!(
        destination.profile(&placed),
        Some((matrix::profile(), None))
    );
    let reached = destination.reach(&placed, "", token());
    assert_eq!(
        reached,
        Some(Reached::new(
            Matrix::new("https://matrix.example/", token(), room)
                .reading_back(ReadBack::DEFAULT)
                .waiting(Waits::DEFAULT)
        ))
    );
    assert_eq!(
        reached.map(|reached| reached.api().to_owned()),
        Some("https://matrix.example".to_owned())
    );
}

#[test]
fn a_destination_is_kept_beside_its_faults_and_a_route_names_a_room() {
    let (checked, faults) = MatrixKind::checked(options(Some(appservice())), &nothing).unwrap();
    assert_eq!(checked, options(Some(appservice())));
    assert_eq!(faults, []);
    let (_, faults) =
        MatrixKind::checked(options(Some(json!({ "prefix": "" }))), &nothing).unwrap();
    assert_eq!(
        faults
            .iter()
            .map(|fault| fault.key.as_deref())
            .collect::<Vec<_>>(),
        [Some("interactions")]
    );
    let placing = Placing {
        destination: "matrix",
        subscribers: false,
    };
    assert_eq!(
        MatrixKind::placed(&options(None), &route(ROOM), placing),
        []
    );
    assert_eq!(
        MatrixKind::placed(&options(None), &route("room"), placing),
        [Fault::here(
            "\"room\" is not a room id; give the room's id, such as \"!abcdefg:matrix.org\", from its settings"
        )]
    );
}

#[test]
fn a_sync_that_waits_as_long_as_herald_waits_for_any_answer_outlasts_it() {
    let synced = options(Some(syncing()));
    assert_eq!(
        MatrixKind::outlasts(&options(None), SignedDuration::ZERO),
        None
    );
    assert_eq!(
        MatrixKind::outlasts(&synced, SYNC_WAIT + SignedDuration::from_secs(1)),
        None
    );
    assert_eq!(
        MatrixKind::outlasts(&synced, SYNC_WAIT),
        Some(Fault::at(
            "interactions",
            "a sync that waits 20s outlasts the 20s [http] total allows any answer; make sync_wait shorter, or total longer"
        ))
    );
}

#[test]
fn commands_are_taken_with_the_tokens_the_homeserver_and_herald_present() {
    let pushed = interacted(&options(Some(appservice())));
    assert_eq!(
        pushed.homeserver_token(),
        Some(&Secret::new("HERALD_MATRIX_HS", "hs-token"))
    );
    assert_eq!(Interacts::api(&pushed), "https://matrix.example");
    assert_eq!(
        Interacts::answering(&pushed),
        Interacting::answering(&pushed)
    );
    assert_eq!(
        pushed.posted(Endpoint::Pushed),
        Some(Ok((crate::SCHEME, "hs-token")))
    );
    assert_eq!(pushed.posted(Endpoint::Posted), None);
    assert!(pushed.session().is_none());
    let synced = interacted(&options(Some(syncing())));
    assert_eq!(
        synced.posted(Endpoint::Pushed),
        Some(Err("application service for such a destination"))
    );
    assert_eq!(
        pollster::block_on(options(None).interacts(&token(), &secrets())),
        Ok(None)
    );
}

#[test]
fn commands_are_answered_in_the_room_they_were_given_in() {
    let pushed = interacted(&options(Some(appservice())));
    let replying = interact::Replying {
        room: ROOM.to_owned(),
        event: "$command".to_owned(),
        thread: None,
    };
    let parts = [Text {
        body: "one".to_owned(),
        html: None,
    }];
    let answered = Interacts::answered(&pushed, &Replying::new(replying.clone()), &parts, true);
    assert_eq!(
        answered,
        Interacting::answered(&pushed, &replying, &parts, true)
    );
    assert_eq!(answered.len(), 1);
    assert_eq!(
        Interacts::answered(&pushed, &Replying::new(()), &parts, true),
        []
    );
    assert!(
        Interacts::took(
            &pushed,
            &response(403, &json!({ "errcode": "M_FORBIDDEN" }))
        )
        .is_err()
    );
    assert_eq!(Interacts::took(&pushed, &response(200, &json!({}))), Ok(()));
}

#[test]
fn a_pushed_transaction_is_delivered_once_and_its_commands_asked() {
    let pushed = interacted(&options(Some(appservice())));
    let path = ["_matrix", "app", "v1", "transactions", "t1"];
    let Read::Delivered {
        delivery,
        replayed,
        then,
    } = read(
        &pushed,
        "PUT",
        &path,
        &json!({ "events": [message("!herald latest")] }),
    )
    else {
        panic!("not delivered");
    };
    assert_eq!(delivery, "matrix/t1");
    assert_eq!(replayed, Said::ok(EMPTY));
    let Read::Asked { asked, answer } = *then else {
        panic!("nothing asked");
    };
    assert_eq!(answer, Said::ok(EMPTY));
    assert_eq!(
        asked
            .iter()
            .map(|(asked, _)| asked.command)
            .collect::<Vec<_>>(),
        [herald_core::Command::Latest]
    );
    assert_eq!(
        read(&pushed, "PUT", &path, &json!({ "events": [message("hi")] })),
        Read::Delivered {
            delivery: "matrix/t1".to_owned(),
            replayed: Said::ok(EMPTY),
            then: Box::new(Read::Said(Said::ok(EMPTY))),
        }
    );
    assert_eq!(
        read(&pushed, "PUT", &path, &json!("nope")),
        Read::Delivered {
            delivery: "matrix/t1".to_owned(),
            replayed: Said::ok(EMPTY),
            then: Box::new(Read::Unread(
                "the body is not a Matrix transaction or sync".to_owned()
            )),
        }
    );
}

#[test]
fn the_homeserver_s_queries_are_answered_for_herald_s_own_user_alone() {
    let pushed = interacted(&options(Some(appservice())));
    let users = |user: &str| {
        read(
            &pushed,
            "GET",
            &["_matrix", "app", "v1", "users", user],
            &Value::Null,
        )
    };
    assert_eq!(users("@herald:matrix.example"), Read::Said(Said::ok(EMPTY)));
    assert_eq!(users("@ann:matrix.example"), Read::Said(unknown()));
    assert_eq!(users("herald"), Read::Said(unknown()));
    assert_eq!(
        read(
            &pushed,
            "GET",
            &["_matrix", "app", "v1", "rooms", "#a:x"],
            &Value::Null
        ),
        Read::Said(unknown())
    );
    assert_eq!(
        read(
            &pushed,
            "POST",
            &["_matrix", "app", "v1", "ping"],
            &json!({})
        ),
        Read::Said(Said::ok(EMPTY))
    );
    for path in [
        ["_matrix", "app", "v1", "transactions", "t1"],
        ["_matrix", "app", "v1", "users", "@herald:x"],
        ["_matrix", "app", "v1", "rooms", "#a:x"],
        ["_matrix", "app", "v1", "ping", "now"],
    ] {
        assert_eq!(
            read(&pushed, "DELETE", &path, &Value::Null),
            Read::Unmethodical,
            "{path:?}"
        );
    }
    assert_eq!(
        read(&pushed, "GET", &["_matrix", "app", "v2"], &Value::Null),
        Read::Missing("such path")
    );
    assert_ne!(unknown(), Said::ok(EMPTY));
}

#[test]
fn a_sync_is_held_through_the_bot_and_asks_what_arrives_after_the_first() {
    let synced = interacted(&options(Some(syncing())));
    let mut session = synced.session().unwrap();
    assert_eq!(session.unreachable(), "the homeserver could not be synced");
    assert_eq!(session.connect(), Connect::Poll(synced.sync(None, 0)));
    let batch = |next: &str| {
        response(
            200,
            &json!({
                "next_batch": next,
                "rooms": { "join": { ROOM: { "timeline": { "events": [message("!herald latest")] } } } },
            }),
        )
    };
    assert_eq!(
        session.polled(&destination(), &batch("s1")),
        Ok(vec![Effect::Ready])
    );
    assert_eq!(
        session.connect(),
        Connect::Poll(synced.sync(Some("s1"), synced.sync_wait()))
    );
    let effects = session.polled(&destination(), &batch("s2")).unwrap();
    assert_eq!(effects.len(), 2);
    assert_eq!(effects[0], Effect::Ready);
    assert!(matches!(effects[1], Effect::Asked(..)), "{effects:?}");
    assert!(
        session
            .polled(
                &destination(),
                &response(401, &json!({ "errcode": "M_UNKNOWN_TOKEN" }))
            )
            .is_err()
    );
    assert_eq!(
        session.polled(&destination(), &response(200, &json!({}))),
        Err("the body is not a Matrix transaction or sync".to_owned())
    );
}
