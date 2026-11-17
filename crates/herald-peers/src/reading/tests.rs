use herald_core::{Event, KindSpec};
use serde_json::{Value, json};

use super::*;
use crate::discovery::Instance;
use crate::feed::{Listed, Page as Written};

const INSTANCE: Instance<'static> = Instance {
    domain: "herald.example",
    base: "/h",
    software: "herald",
    version: "1.2.3",
    repository: "https://github.com/nightworksio/herald",
};

const FEED: &str = "https://herald.example/h/herald/v1/feeds/releases";

fn event(origin: bool) -> Event {
    let mut written = json!({
        "identity": "forge.example/1/v1",
        "kind": "release",
        "project": "gate",
        "title": "gate v1",
        "body": "the gate",
        "links": { "release": "https://forge.example/gate/releases/v1" },
        "attributes": { "version": "v1" },
        "occurred": "2026-11-17T10:00:00Z",
    });
    if origin {
        written["origin"] = json!({
            "forge": "github",
            "host": "forge.example",
            "owner": "acme",
            "repository": "acme/gate",
            "id": "1",
            "visibility": "public",
        });
    }
    serde_json::from_value(written).unwrap()
}

fn envelope(origin: bool) -> Value {
    INSTANCE
        .envelope(&event(origin), Some(&KindSpec::shipped().remove(0)), 8192)
        .unwrap()
}

fn written(items: &[(u64, Value)], before: Option<u64>, more: bool) -> String {
    Written {
        feed: FEED,
        before,
        listed: items
            .iter()
            .map(|(sequence, envelope)| Listed {
                sequence: *sequence,
                envelope,
            })
            .collect(),
        more,
    }
    .document()
}

#[test]
fn a_page_herald_wrote_reads_back_newest_first() {
    let items = vec![(9, envelope(true)), (4, envelope(false))];
    let read = page(written(&items, None, true).as_bytes(), FEED).unwrap();
    assert_eq!(read, Page { items, more: true });
    let asked = format!("{FEED}?before=4");
    let last = page(written(&[], Some(4), false).as_bytes(), &asked).unwrap();
    assert_eq!(
        last,
        Page {
            items: Vec::new(),
            more: false
        }
    );
}

#[test]
fn a_page_is_refused_unless_it_is_the_one_asked_in_herald_s_shape() {
    let one = [(1, envelope(false))];
    assert_eq!(page(b"{", FEED), Err(Unread::Json));
    assert_eq!(
        page(br#"{"items":[]}"#, FEED),
        Err(Unread::Shape("it names no id"))
    );
    assert_eq!(
        page(written(&one, Some(2), false).as_bytes(), FEED),
        Err(Unread::Elsewhere(format!("{FEED}?before=2")))
    );
    let shaped = |items: Value| json!({ "id": FEED, "items": items }).to_string();
    assert_eq!(
        page(json!({ "id": FEED }).to_string().as_bytes(), FEED),
        Err(Unread::Shape("it lists no items"))
    );
    assert_eq!(
        page(shaped(json!([{ "envelope": {} }])).as_bytes(), FEED),
        Err(Unread::Shape("an item has no sequence"))
    );
    assert_eq!(
        page(
            shaped(json!([{ "sequence": 1, "envelope": [] }])).as_bytes(),
            FEED
        ),
        Err(Unread::Shape("an item holds no envelope"))
    );
    assert_eq!(
        page(shaped(json!([{ "sequence": 1 }])).as_bytes(), FEED),
        Err(Unread::Shape("an item holds no envelope"))
    );
    assert_eq!(
        page(shaped(json!({ "next": 1 })).as_bytes(), FEED),
        Err(Unread::Shape("it lists no items"))
    );
    let numbered = json!({ "id": FEED, "items": [], "next": 7 }).to_string();
    assert!(!page(numbered.as_bytes(), FEED).unwrap().more);
}

#[test]
fn a_page_lists_at_most_as_many_as_any_page_does_newest_first() {
    let item = |sequence: u64| (sequence, json!({ "a": 1 }));
    let full: Vec<(u64, Value)> = (1..=PAGE_MOST as u64).rev().map(item).collect();
    assert_eq!(
        page(written(&full, None, false).as_bytes(), FEED)
            .unwrap()
            .items
            .len(),
        PAGE_MOST
    );
    let crowded: Vec<(u64, Value)> = (1..=PAGE_MOST as u64 + 1).rev().map(item).collect();
    assert_eq!(
        page(written(&crowded, None, false).as_bytes(), FEED),
        Err(Unread::Crowded(PAGE_MOST + 1))
    );
    assert_eq!(
        page(written(&[item(2), item(2)], None, false).as_bytes(), FEED),
        Err(Unread::Unordered)
    );
    assert_eq!(
        page(written(&[item(2), item(3)], None, false).as_bytes(), FEED),
        Err(Unread::Unordered)
    );
}

#[test]
fn an_envelope_takes_at_most_the_bytes_any_envelope_may() {
    let wrapped = |text: String| json!({ "t": text });
    let room = ENVELOPE_MOST - canonical(&wrapped(String::new())).unwrap().len();
    let fits = wrapped("a".repeat(room));
    assert!(page(written(&[(1, fits)], None, false).as_bytes(), FEED).is_ok());
    let over = wrapped("a".repeat(room + 1));
    assert_eq!(
        page(written(&[(1, over)], None, false).as_bytes(), FEED),
        Err(Unread::Oversized(ENVELOPE_MOST + 1))
    );
}

#[test]
fn an_envelope_reads_back_as_the_event_it_carries() {
    let Carried::Event(read) = carried(&envelope(true)).unwrap() else {
        panic!("an event");
    };
    assert_eq!(
        *read,
        Enveloped {
            identity: "forge.example/1/v1".to_owned(),
            kind: "release".to_owned(),
            project: "gate".to_owned(),
            title: "gate v1".to_owned(),
            body: "the gate".to_owned(),
            links: [(
                "release".to_owned(),
                "https://forge.example/gate/releases/v1".to_owned()
            )]
            .into(),
            attributes: json!({ "version": "v1" }).as_object().unwrap().clone(),
            occurred: "2026-11-17T10:00:00Z".parse().unwrap(),
            origin: Some([
                "github".to_owned(),
                "forge.example".to_owned(),
                "acme/gate".to_owned()
            ]),
            path: vec!["https://herald.example/h/.well-known/herald".to_owned()],
        }
    );
    let Carried::Event(bare) = carried(&envelope(false)).unwrap() else {
        panic!("an event");
    };
    assert_eq!(bare.origin, None);
}

#[test]
fn a_tombstone_reads_back_as_the_identity_it_withdraws() {
    let tombstone = INSTANCE.tombstone(
        "forge.example/1/v1",
        "2026-11-17T10:00:00Z".parse().unwrap(),
    );
    assert_eq!(
        carried(&tombstone),
        Ok(Carried::Withdrawn {
            identity: "forge.example/1/v1".to_owned(),
            path: vec!["https://herald.example/h/.well-known/herald".to_owned()],
        })
    );
}

#[test]
fn an_envelope_out_of_shape_or_not_public_is_refused() {
    let with = |name: &str, value: Value| {
        let mut changed = envelope(true);
        changed[name] = value;
        carried(&changed)
    };
    let without = |name: &str| {
        let mut changed = envelope(true);
        changed.as_object_mut().unwrap().remove(name);
        carried(&changed)
    };
    let shape = |what| Err(Unread::Shape(what));
    assert_eq!(without("path"), shape("it names no path of instances"));
    assert_eq!(
        with("path", json!([])),
        shape("it names no path of instances")
    );
    assert_eq!(
        with("path", json!([1])),
        shape("it names no path of instances")
    );
    assert_eq!(without("identity"), shape("it names no identity"));
    assert_eq!(
        with("type", json!("herald:Other")),
        shape("it is neither an event nor a tombstone")
    );
    assert_eq!(
        without("type"),
        shape("it is neither an event nor a tombstone")
    );
    assert_eq!(with("visibility", json!("private")), Err(Unread::Unpublic));
    assert_eq!(without("visibility"), Err(Unread::Unpublic));
    assert_eq!(without("kind"), shape("it names no kind"));
    assert_eq!(without("project"), shape("it names no project"));
    assert_eq!(without("title"), shape("it has no title"));
    assert_eq!(without("body"), shape("it has no body"));
    assert_eq!(
        with("links", json!([])),
        shape("its links are not named URLs")
    );
    assert_eq!(
        with("links", json!({ "release": 1 })),
        shape("its links are not named URLs")
    );
    assert_eq!(
        with("occurred", json!("then")),
        shape("it names no time it happened")
    );
    assert_eq!(without("occurred"), shape("it names no time it happened"));
    assert_eq!(
        with("attributes", json!([])),
        shape("its attributes are not an object")
    );
    for missing in ["forge", "host", "repository"] {
        let mut origin =
            json!({ "forge": "github", "host": "forge.example", "repository": "acme/gate" });
        origin.as_object_mut().unwrap().remove(missing);
        assert_eq!(
            with("origin", origin),
            shape("its origin names no forge, host and repository")
        );
    }
}

#[test]
fn every_refusal_reads_as_a_sentence() {
    for (unread, said) in [
        (Unread::Json, "the feed's page is no JSON".to_owned()),
        (
            Unread::Shape("it names no id"),
            "the feed's page cannot be read: it names no id".to_owned(),
        ),
        (
            Unread::Elsewhere("https://x".to_owned()),
            "the feed answered with the page https://x, not the one asked".to_owned(),
        ),
        (
            Unread::Crowded(81),
            "the feed's page lists 81 envelopes, more than the 80 any page may".to_owned(),
        ),
        (
            Unread::Unordered,
            "the feed's page does not list its envelopes newest first".to_owned(),
        ),
        (
            Unread::Oversized(70_000),
            "an envelope takes 70000 bytes, more than the 65536 any may".to_owned(),
        ),
        (
            Unread::Unpublic,
            "an envelope carries an event that is not public".to_owned(),
        ),
    ] {
        assert_eq!(unread.to_string(), said);
    }
    let inexact = format!(
        r#"{{"id":"{FEED}","items":[{{"sequence":1,"envelope":{{"n":9007199254740993}}}}]}}"#
    );
    let refused = page(inexact.as_bytes(), FEED).unwrap_err();
    assert!(matches!(refused, Unread::Uncanonical(_)));
    assert!(
        refused
            .to_string()
            .starts_with("the integer 9007199254740993")
    );
}
