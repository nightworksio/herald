use herald_core::Replies;

use super::*;

fn interactions(extra: &str) -> Interactions {
    serde_json::from_str(&format!(
        r#"{{"homeserver_token": {{"secret": "HS"}}, "appservice_token": {{"secret": "AS"}}{extra}}}"#
    ))
    .unwrap()
}

fn interacting(extra: &str) -> Interacting {
    Interacting::new(
        "https://matrix.example/",
        interactions(extra),
        Secret::new("HERALD_MATRIX_AS", "as-token"),
    )
}

fn destination() -> DestinationName {
    DestinationName::new("matrix")
}

fn message(body: &str, sender: &str) -> Value {
    json!({
        "type": "m.room.message",
        "room_id": "!room:matrix.example",
        "event_id": "$command",
        "sender": sender,
        "content": { "msgtype": "m.text", "body": body },
    })
}

fn pushed(events: &[Value]) -> Vec<(Asked, Replying)> {
    interacting("")
        .transaction(
            &destination(),
            json!({ "events": events }).to_string().as_bytes(),
        )
        .unwrap()
}

#[test]
fn a_command_in_a_transaction_is_read_with_its_project_and_share() {
    let asked = pushed(&[message(
        "!herald latest herald share",
        "@ann:matrix.example",
    )]);
    assert_eq!(
        asked,
        [(
            Asked {
                destination: destination(),
                place: Place::new("!room:matrix.example"),
                user: "@ann:matrix.example".to_owned(),
                command: Command::Latest,
                project: Some("herald".to_owned()),
                share: true,
                toggles: false,
                member: None,
            },
            Replying {
                room: "!room:matrix.example".to_owned(),
                event: "$command".to_owned(),
                thread: None,
            }
        )]
    );
    let mut threaded = message("!herald latest", "@ann:x");
    threaded["content"]["m.relates_to"] = json!({ "rel_type": "m.thread", "event_id": "$root" });
    let mut replied = message("!herald latest", "@ann:x");
    replied["content"]["m.relates_to"] = json!({ "m.in_reply_to": { "event_id": "$other" } });
    let threads: Vec<Option<String>> = pushed(&[threaded, replied])
        .into_iter()
        .map(|(_, replying)| replying.thread)
        .collect();
    assert_eq!(threads, [Some("$root".to_owned()), None]);
    let plain = pushed(&[message("!herald Subscribe", "@ann:x")]);
    assert_eq!(
        (
            plain[0].0.command,
            plain[0].0.project.clone(),
            plain[0].0.share
        ),
        (Command::Subscribe, None, false)
    );
    let shared = pushed(&[message("!herald changelog share", "@ann:x")]);
    assert_eq!(
        (shared[0].0.project.clone(), shared[0].0.share),
        (None, true)
    );
    let named = pushed(&[message("!herald latest gate", "@ann:x")]);
    assert_eq!(
        (named[0].0.project.as_deref(), named[0].0.share),
        (Some("gate"), false)
    );
}

#[test]
fn messages_that_are_not_commands_to_herald_are_passed_over() {
    let mut notice = message("!herald latest", "@ann:x");
    notice["content"]["msgtype"] = json!("m.notice");
    let mut state = message("!herald latest", "@ann:x");
    state["type"] = json!("m.room.member");
    let mut roomless = message("!herald latest", "@ann:x");
    roomless.as_object_mut().unwrap().remove("room_id");
    let mut idless = message("!herald latest", "@ann:x");
    idless.as_object_mut().unwrap().remove("event_id");
    let mut contentless = message("!herald latest", "@ann:x");
    contentless.as_object_mut().unwrap().remove("content");
    let mut unnamed_room = message("!herald latest", "@ann:x");
    unnamed_room["room_id"] = json!("!");
    let mut aliased = message("!herald latest", "@ann:x");
    aliased["room_id"] = json!("#room:x");
    let events = [
        message("hello", "@ann:x"),
        message("!heraldlatest", "@ann:x"),
        message("!herald", "@ann:x"),
        message("!herald deploy", "@ann:x"),
        message("!herald latest a b", "@ann:x"),
        message("!herald latest", "@herald:matrix.example"),
        message("!herald latest", ""),
        message("!herald latest", "@"),
        message("!herald latest", "ann:x"),
        unnamed_room,
        aliased,
        notice,
        state,
        roomless,
        idless,
        contentless,
    ];
    assert_eq!(pushed(&events), []);
}

#[test]
fn a_body_that_is_no_transaction_is_unread() {
    assert_eq!(
        interacting("").transaction(&destination(), b"{}"),
        Err(Unread::NotEvents)
    );
    assert_eq!(
        interacting("").transaction(&destination(), b"nope"),
        Err(Unread::NotEvents)
    );
    assert_eq!(
        Unread::NotEvents.to_string(),
        "the body is not a Matrix transaction or sync"
    );
}

#[test]
fn a_sync_s_commands_are_read_from_each_joined_room_with_where_the_next_starts() {
    let body = json!({
        "next_batch": "s2",
        "rooms": { "join": {
            "!a:x": { "timeline": { "events": [message("!herald latest", "@ann:x")] } },
            "!b:x": { "timeline": { "events": [message("hi", "@ann:x")] } },
            "!c:x": {},
        }},
    });
    let synced = interacting("")
        .synced(&destination(), body.to_string().as_bytes())
        .unwrap();
    assert_eq!(synced.next, "s2");
    assert_eq!(synced.asked.len(), 1);
    assert_eq!(synced.asked[0].0.place, Place::new("!a:x"));
    assert_eq!(synced.asked[0].1.room, "!a:x");
    let quiet = interacting("")
        .synced(&destination(), br#"{"next_batch": "s3"}"#)
        .unwrap();
    assert_eq!(
        quiet,
        Synced {
            next: "s3".to_owned(),
            asked: Vec::new()
        }
    );
    assert_eq!(
        interacting("").synced(&destination(), b"{}"),
        Err(Unread::NotEvents)
    );
    assert_eq!(
        interacting("").synced(&destination(), b"x"),
        Err(Unread::NotEvents)
    );
}

#[test]
fn a_sync_waits_from_where_the_last_left_off_with_herald_s_filter() {
    let first = interacting("").sync(None, 0);
    assert_eq!(first.method, Method::Get);
    let url = first.url.expose();
    assert!(
        url.starts_with(
            "https://matrix.example/_matrix/client/v3/sync?timeout=0&filter=%7B%22presence%22"
        ),
        "{url}"
    );
    assert!(!url.contains("since"));
    let next = interacting("").sync(Some("s 2"), SYNC_WAIT).url.expose();
    assert!(next.contains("timeout=20000"));
    assert!(next.ends_with("&since=s%202"), "{next}");
    let header = first
        .headers
        .iter()
        .find(|(name, _)| name == "Authorization")
        .map(|(_, value)| value.expose());
    assert_eq!(header.as_deref(), Some("Bearer as-token"));
}

#[test]
fn an_answer_is_threaded_under_the_command_unless_public_once_per_part() {
    let replying = Replying {
        room: "!room:matrix.example".to_owned(),
        event: "$command".to_owned(),
        thread: None,
    };
    let parts = [
        Written {
            body: "one".to_owned(),
            html: Some("<p>one</p>".to_owned()),
        },
        Written {
            body: "two".to_owned(),
            html: None,
        },
    ];
    let threaded = interacting("").answered(&replying, &parts, false);
    assert_eq!(threaded.len(), 2);
    assert_eq!(threaded[0].method, Method::Put);
    let drawn: String = digest("$command").chars().take(32).collect();
    assert_eq!(
        threaded[0].url.expose(),
        format!(
            "https://matrix.example/_matrix/client/v3/rooms/%21room%3Amatrix.example/send/m.room.message/herald.{drawn}.0"
        )
    );
    assert!(threaded[1].url.expose().ends_with(".1"));
    assert_eq!(
        serde_json::from_slice::<Value>(&threaded[0].body).unwrap(),
        json!({
            "msgtype": "m.notice",
            "body": "one",
            "format": "org.matrix.custom.html",
            "formatted_body": "<p>one</p>",
            "m.mentions": {},
            "m.relates_to": {
                "rel_type": "m.thread",
                "event_id": "$command",
                "is_falling_back": true,
                "m.in_reply_to": { "event_id": "$command" },
            },
        })
    );
    let plain = serde_json::from_slice::<Value>(&threaded[1].body).unwrap();
    assert_eq!(plain.get("format"), None);
    let public = interacting("").answered(&replying, &parts, true);
    assert_eq!(
        serde_json::from_slice::<Value>(&public[0].body).unwrap()["m.relates_to"],
        json!({ "m.in_reply_to": { "event_id": "$command" } })
    );
}

#[test]
fn an_answer_to_a_command_given_in_a_thread_replies_to_it_in_that_thread_public_or_not() {
    let replying = Replying {
        room: "!room:matrix.example".to_owned(),
        event: "$command".to_owned(),
        thread: Some("$root".to_owned()),
    };
    let parts = [Written {
        body: "one".to_owned(),
        html: None,
    }];
    for public in [false, true] {
        let answered = interacting("").answered(&replying, &parts, public);
        assert_eq!(
            serde_json::from_slice::<Value>(&answered[0].body).unwrap()["m.relates_to"],
            json!({
                "rel_type": "m.thread",
                "event_id": "$root",
                "is_falling_back": false,
                "m.in_reply_to": { "event_id": "$command" },
            }),
            "{public}"
        );
    }
}

#[test]
fn the_registration_owns_the_sender_alone_with_its_tokens_quoted() {
    let registration = interacting(r#", "sender": "her.ald""#).registration(
        "https://herald.example",
        "hs'1",
        "as2",
    );
    assert_eq!(
        registration,
        "id: 'herald'\nurl: 'https://herald.example'\nas_token: 'as2'\nhs_token: 'hs''1'\nsender_localpart: 'her.ald'\nrate_limited: false\nnamespaces:\n  users:\n    - exclusive: true\n      regex: '@her\\.ald:.*'\n  rooms: []\n  aliases: []\n"
    );
}

#[test]
fn the_homeserver_taking_an_answer_or_not_is_read_from_its_status() {
    let answer = |status, body: &str| Response {
        status,
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
    };
    assert_eq!(took(&answer(200, "{}")), Ok(()));
    assert_eq!(
        took(&answer(
            403,
            r#"{"errcode": "M_FORBIDDEN", "error": "not in room"}"#
        )),
        Err("the homeserver answered 403: M_FORBIDDEN not in room".to_owned())
    );
    assert_eq!(
        took(&answer(502, "")),
        Err("the homeserver answered 502".to_owned())
    );
}

#[test]
fn the_destination_says_where_it_reaches_and_how_it_answers() {
    let interacting = interacting(r#", "replies": "public", "via": "sync""#);
    assert_eq!(interacting.api(), "https://matrix.example");
    assert_eq!(interacting.via(), Via::Sync);
    assert_eq!(interacting.sender(), "herald");
    assert_eq!(interacting.answering().replies, Replies::Public);
}
