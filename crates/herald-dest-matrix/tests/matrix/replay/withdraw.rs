//! A Matrix post withdrawn (ADR-0048): unpinned where the route pinned it,
//! then redacted.

use std::collections::BTreeMap;

use herald_core::{Entry, Facts, Sent, Step};
use herald_dest_matrix::{Matrix, RouteOptions};
use herald_ports::Secret;
use herald_testing::sample::announcement;
use herald_testing::{Answer, Exchange, Fixture, Recorded, Replay};
use serde_json::{Value, json};

const ROOM: &str = "!abcdefg:matrix.example";
const PINS: &str = "https://matrix.example/_matrix/client/v3/rooms/%21abcdefg%3Amatrix.example/state/m.room.pinned_events";

fn matrix() -> Matrix {
    Matrix::new(
        "https://matrix.example/",
        Secret::new("HERALD_MATRIX_TOKEN", "test-token"),
        RouteOptions {
            room: ROOM.to_owned(),
        },
    )
}

fn pinned() -> Entry {
    Entry::new(announcement().key)
        .landed(
            Step::Post,
            Facts {
                message: Some("$post".to_owned()),
                ..Facts::default()
            },
        )
        .landed(Step::Pin, Facts::default())
}

fn answer(status: u16, body: Value) -> Answer {
    Answer {
        status,
        headers: BTreeMap::new(),
        body: Some(body),
    }
}

fn exchange(request: Recorded, response: Answer) -> Exchange {
    Exchange { request, response }
}

fn read_pins(entry: &Entry) -> Recorded {
    Recorded::of(
        &matrix()
            .request(&Step::Pin, &announcement(), entry)
            .unwrap(),
    )
}

fn write_pins(entry: &Entry, pins: &[&str]) -> Recorded {
    let mut written = read_pins(entry);
    written.method = "PUT".to_owned();
    written
        .headers
        .insert("Content-Type".to_owned(), "application/json".to_owned());
    written.body = Some(json!({ "pinned": pins }));
    written
}

fn redact(entry: &Entry) -> Recorded {
    Recorded::of(
        &matrix()
            .request(&Step::Delete(0), &announcement(), entry)
            .unwrap(),
    )
}

fn withdrawn(entry: &Entry, exchanges: Vec<Exchange>) -> Sent {
    let replay = Replay::of(Fixture { exchanges });
    let sent = pollster::block_on(matrix().send(&replay, &Step::Delete(0), &announcement(), entry))
        .unwrap();
    replay.finish().unwrap();
    sent
}

#[test]
fn a_pinned_post_is_unpinned_others_kept_before_it_is_redacted() {
    let entry = pinned();
    assert_eq!(read_pins(&entry).url, PINS);
    let redacted = answer(200, json!({ "event_id": "$redaction" }));
    let sent = withdrawn(
        &entry,
        vec![
            exchange(
                read_pins(&entry),
                answer(200, json!({ "pinned": ["$other", "$post"] })),
            ),
            exchange(
                write_pins(&entry, &["$other"]),
                answer(200, json!({ "event_id": "$pins" })),
            ),
            exchange(redact(&entry), redacted.clone()),
        ],
    );
    assert!(matches!(sent, Sent::Landed(_)));
    let unpinned = withdrawn(
        &entry,
        vec![
            exchange(
                read_pins(&entry),
                answer(200, json!({ "pinned": ["$other"] })),
            ),
            exchange(redact(&entry), redacted.clone()),
        ],
    );
    assert!(matches!(unpinned, Sent::Landed(_)));
    let never = Entry::new(announcement().key).landed(
        Step::Post,
        Facts {
            message: Some("$post".to_owned()),
            ..Facts::default()
        },
    );
    let alone = withdrawn(&never, vec![exchange(redact(&never), redacted)]);
    assert!(matches!(alone, Sent::Landed(_)));
}

#[test]
fn a_post_whose_pins_cannot_be_read_or_written_is_not_redacted() {
    let entry = pinned();
    let forbidden = || answer(403, json!({ "errcode": "M_FORBIDDEN", "error": "no" }));
    let unread = withdrawn(&entry, vec![exchange(read_pins(&entry), forbidden())]);
    assert!(matches!(unread, Sent::Refused(_)), "{unread:?}");
    let unwritten = withdrawn(
        &entry,
        vec![
            exchange(
                read_pins(&entry),
                answer(200, json!({ "pinned": ["$post"] })),
            ),
            exchange(write_pins(&entry, &[]), forbidden()),
        ],
    );
    assert!(matches!(unwritten, Sent::Refused(_)), "{unwritten:?}");
}
