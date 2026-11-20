use herald_core::{Event, KindSpec};
use serde_json::{Value, json};

use super::*;
use crate::jcs::canonical;

const INSTANCE: Instance<'static> = Instance {
    domain: "herald.example",
    base: "",
    software: "herald",
    version: "1.2.3",
    repository: "https://github.com/nightworksio/herald",
};

fn event(title: &str, body: &str, release: bool) -> Event {
    let links = if release {
        json!({ "release": "https://forge.example/gate/releases/v1" })
    } else {
        json!({ "changelog": "https://forge.example/gate/CHANGELOG.md" })
    };
    serde_json::from_value(json!({
        "identity": "forge.example/1/v1",
        "kind": "release",
        "project": "gate",
        "title": title,
        "body": body,
        "links": links,
        "attributes": { "version": "v1" },
        "occurred": "2026-11-17T10:00:00Z",
    }))
    .unwrap()
}

fn release() -> KindSpec {
    KindSpec::shipped().remove(0)
}

fn size(envelope: &Value) -> usize {
    canonical(envelope).unwrap().len()
}

#[test]
fn an_envelope_is_what_of_the_event_is_public_its_type_its_path_and_its_evidence() {
    let released = event("gate v1", "the gate", true);
    let envelope = INSTANCE
        .envelope((&released, Visibility::Public), Some(&release()), 8192)
        .unwrap();
    assert_eq!(
        envelope,
        json!({
            "identity": "forge.example/1/v1",
            "kind": "release",
            "project": "gate",
            "title": "gate v1",
            "body": "the gate",
            "links": { "release": "https://forge.example/gate/releases/v1" },
            "attributes": { "version": "v1" },
            "occurred": "2026-11-17T10:00:00Z",
            "visibility": "public",
            "type": "herald:Event",
            "path": ["https://herald.example/.well-known/herald"],
            "evidence": { "release": "https://forge.example/gate/releases/v1" },
        })
    );
    let unreleased = INSTANCE
        .envelope(
            (&event("gate v1", "the gate", false), Visibility::Public),
            Some(&release()),
            8192,
        )
        .unwrap();
    assert_eq!(unreleased.get("evidence"), None);
    assert_eq!(unreleased.get("cut"), None);
}

#[test]
fn an_envelope_carries_no_field_the_event_keeps_to_itself() {
    let mut written = serde_json::to_value(event("gate v1", "b", true)).unwrap();
    written["attributes"]["signal"] = json!("release");
    written["visibility"] = json!("public");
    written["origin"] = json!({
        "forge": "github",
        "host": "forge.example",
        "owner": "nightworksio",
        "repository": "nightworksio/gate",
        "id": "1",
        "topics": ["internal-topic"],
        "properties": { "team": ["secret"] },
        "visibility": "public",
    });
    let carried: Event = serde_json::from_value(written).unwrap();
    let envelope = INSTANCE
        .envelope((&carried, Visibility::Public), Some(&release()), 8192)
        .unwrap();
    let keys: Vec<&str> = envelope
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "attributes",
            "body",
            "evidence",
            "identity",
            "kind",
            "links",
            "occurred",
            "origin",
            "path",
            "project",
            "title",
            "type",
            "visibility",
        ]
    );
    assert_eq!(
        envelope["origin"],
        json!({ "forge": "github", "host": "forge.example", "repository": "nightworksio/gate" })
    );
    assert_eq!(
        envelope["attributes"],
        json!({ "signal": "release", "version": "v1" })
    );
    let undeclared = INSTANCE
        .envelope((&carried, Visibility::Public), None, 8192)
        .unwrap();
    assert_eq!(undeclared["attributes"], json!({}));
}

#[test]
fn an_envelope_that_fits_exactly_keeps_its_body() {
    let released = event("gate v1", "the whole body", true);
    let whole = size(
        &INSTANCE
            .envelope(
                (&released, Visibility::Public),
                Some(&release()),
                usize::MAX,
            )
            .unwrap(),
    );
    let fitting = INSTANCE
        .envelope(
            (&released, Visibility::Public),
            Some(&release()),
            whole + PROOF_ROOM,
        )
        .unwrap();
    assert_eq!(fitting["body"], json!("the whole body"));
    assert_eq!(fitting.get("cut"), None);
    let cut = INSTANCE
        .envelope(
            (&released, Visibility::Public),
            Some(&release()),
            whole + PROOF_ROOM - 1,
        )
        .unwrap();
    assert_eq!(cut["cut"], json!(true));
}

#[test]
fn a_long_body_is_cut_to_fill_the_room_left_for_the_proof() {
    let body = "a".repeat(4000);
    let envelope = INSTANCE
        .envelope(
            (&event("gate v1", &body, true), Visibility::Public),
            Some(&release()),
            2048,
        )
        .unwrap();
    assert_eq!(envelope["cut"], json!(true));
    assert_eq!(size(&envelope), 2048 - PROOF_ROOM);
    let kept = envelope["body"].as_str().unwrap();
    assert!(body.starts_with(kept) && kept.len() > 1000, "{kept}");
}

#[test]
fn a_body_is_cut_between_characters() {
    let body = "é".repeat(2000);
    let envelope = INSTANCE
        .envelope(
            (&event("gate v1", &body, true), Visibility::Public),
            Some(&release()),
            2048,
        )
        .unwrap();
    let kept = envelope["body"].as_str().unwrap();
    assert!(body.starts_with(kept) && !kept.is_empty(), "{kept}");
    assert!(size(&envelope) <= 2048 - PROOF_ROOM);
    assert!(size(&envelope) > 2048 - PROOF_ROOM - 2);
}

#[test]
fn an_envelope_too_long_without_its_body_is_refused() {
    let title = "t".repeat(3000);
    let refused = INSTANCE.envelope(
        (&event(&title, "body", true), Visibility::Public),
        Some(&release()),
        2048,
    );
    let Err(Unenveloped::Oversized { bytes, most }) = refused else {
        panic!("{refused:?}");
    };
    assert_eq!(most, 2048 - PROOF_ROOM);
    assert!(bytes > 3000 && bytes < 3400, "{bytes}");
    let inexact = crate::jcs::Uncanonical::Inexact {
        number: "1e400".to_owned(),
    };
    assert_eq!(
        Unenveloped::Uncanonical(inexact.clone()).to_string(),
        inexact.to_string()
    );
    assert!(
        Unenveloped::Oversized { bytes: 9, most: 4 }
            .to_string()
            .contains("takes 9 bytes with its body cut, more than the 4")
    );
}

#[test]
fn an_event_whose_identity_or_project_is_too_long_has_no_envelope() {
    let at = |field: &str, bytes: usize| {
        let mut long = serde_json::to_value(event("gate v1", "body", true)).unwrap();
        long[field] = json!("i".repeat(bytes));
        let long: Event = serde_json::from_value(long).unwrap();
        INSTANCE.envelope((&long, Visibility::Public), Some(&release()), ENVELOPE_MOST)
    };
    for (field, most) in [("identity", IDENTITY_MOST), ("project", PROJECT_MOST)] {
        assert!(at(field, most).is_ok(), "{field}");
        assert_eq!(
            at(field, most + 1),
            Err(Unenveloped::Long(Long {
                what: field,
                bytes: most + 1,
                most
            }))
        );
    }
    assert_eq!(
        Unenveloped::Long(Long::of("identity", &"i".repeat(1025), IDENTITY_MOST).unwrap())
            .to_string(),
        "the event's identity takes 1025 bytes, more than the 1024 an envelope's may"
    );
    assert_eq!(Long::of("project", "p", 1), None);
}

#[test]
fn a_feed_lives_under_the_api() {
    let based = Instance {
        base: "/b",
        ..INSTANCE
    };
    assert_eq!(
        based.feed("releases"),
        "https://herald.example/b/herald/v1/feeds/releases"
    );
    assert_eq!(
        based.feed("night works/1"),
        "https://herald.example/b/herald/v1/feeds/night%20works%2F1"
    );
}

#[test]
fn a_page_lists_its_envelopes_newest_first_and_links_the_next() {
    let (newer, older) = (json!({ "n": 2 }), json!({ "n": 1 }));
    let feed = "https://herald.example/herald/v1/feeds/releases";
    let page = Page {
        feed,
        before: None,
        listed: vec![
            Listed {
                sequence: 9,
                envelope: &newer,
            },
            Listed {
                sequence: 7,
                envelope: &older,
            },
        ],
        more: true,
    };
    assert_eq!(
        serde_json::from_str::<Value>(&page.document()).unwrap(),
        json!({
            "id": feed,
            "feed": feed,
            "items": [
                { "sequence": 9, "envelope": { "n": 2 } },
                { "sequence": 7, "envelope": { "n": 1 } },
            ],
            "next": format!("{feed}?before=7"),
        })
    );
    let last = Page {
        before: Some(7),
        more: false,
        ..page.clone()
    };
    let written: Value = serde_json::from_str(&last.document()).unwrap();
    assert_eq!(written["id"], json!(format!("{feed}?before=7")));
    assert_eq!(written.get("next"), None);
    let empty = Page {
        listed: Vec::new(),
        more: true,
        ..page
    };
    let written: Value = serde_json::from_str(&empty.document()).unwrap();
    assert_eq!(written["items"], json!([]));
    assert_eq!(written.get("next"), None);
}

#[test]
fn a_page_s_tag_changes_with_the_feed_the_cursor_and_the_page_size() {
    assert_eq!(tag(9, None, 20), "\"9.0.20\"");
    assert_eq!(tag(9, Some(4), 20), "\"9.4.20\"");
    assert_ne!(tag(10, None, 20), tag(9, None, 20));
    assert_ne!(tag(9, None, 21), tag(9, None, 20));
}

#[test]
fn a_tombstone_withdraws_an_event_s_envelope_by_its_identity() {
    let tombstone = INSTANCE.tombstone(
        "forge.example/1/v1",
        jiff::Timestamp::constant(1_793_448_000, 0),
    );
    assert_eq!(
        tombstone,
        json!({
            "type": "herald:Tombstone",
            "identity": "forge.example/1/v1",
            "deleted": "2026-10-31T12:00:00Z",
            "path": ["https://herald.example/.well-known/herald"],
        })
    );
    assert!(withdrawn(&tombstone));
    let envelope = INSTANCE
        .envelope(
            (&event("gate v1", "b", true), Visibility::Public),
            None,
            8192,
        )
        .unwrap();
    assert!(!withdrawn(&envelope));
    assert!(!withdrawn(&json!({ "type": 1 })));
}

#[test]
fn a_listed_envelope_shows_what_it_carries_whatever_its_proof() {
    let envelope = json!({ "identity": "forge.example/1/v1", "n": 1 });
    let proved = json!({ "identity": "forge.example/1/v1", "n": 1, "proof": { "a": 1 } });
    assert!(shows(&proved, &envelope));
    assert!(shows(&envelope, &envelope));
    assert!(!shows(
        &proved,
        &json!({ "identity": "forge.example/1/v1", "n": 2 })
    ));
    assert!(!shows(&json!([1]), &envelope));
    assert_eq!(identity(&proved), "forge.example/1/v1");
    assert_eq!(identity(&json!({ "identity": 1 })), "");
}

#[test]
fn a_feed_s_url_names_the_instance_it_is_served_by() {
    let url = feed_of("https://herald.example/h", "releases/all");
    assert_eq!(
        url,
        "https://herald.example/h/herald/v1/feeds/releases%2Fall"
    );
    assert_eq!(
        feed_at(&url),
        Some(FeedAt {
            feed: url.clone(),
            instance: "https://herald.example/h".to_owned(),
            domain: "herald.example".to_owned(),
        })
    );
    assert_eq!(
        feed_at("https://herald.example/herald/v1/feeds/releases/"),
        Some(FeedAt {
            feed: "https://herald.example/herald/v1/feeds/releases".to_owned(),
            instance: "https://herald.example".to_owned(),
            domain: "herald.example".to_owned(),
        })
    );
    for other in [
        "https://herald.example/herald/v1/feeds/",
        "https://herald.example/herald/v1/feeds/a/b",
        "https://herald.example/herald/v1/releases",
        "http://herald.example/herald/v1/feeds/releases",
    ] {
        assert_eq!(feed_at(other), None, "{other}");
    }
}

#[test]
fn an_envelope_made_here_names_this_instance_alone_as_its_path() {
    let here = "https://herald.example/.well-known/herald";
    let made = made_at(
        br#"{"identity": "github.com/1/v1", "path": ["https://elsewhere.example/.well-known/herald"]}"#,
        here,
    )
    .unwrap();
    assert_eq!(
        made,
        json!({ "identity": "github.com/1/v1", "path": [here] })
    );
    for unread in [b"[1]".as_slice(), b"not json"] {
        assert!(made_at(unread, here).is_err());
    }
}
