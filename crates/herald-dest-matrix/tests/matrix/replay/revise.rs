//! A Matrix post brought to a new revision: each part replaced by an edit
//! event, and each reply no longer needed redacted.

use herald_core::{Entry, Facts, Sent, Step, digest, unreserved};
use herald_dest_matrix::{Matrix, RouteOptions};
use herald_ports::Secret;
use herald_testing::sample::announcement;
use herald_testing::{Answer, Exchange, Fixture, Recorded, Replay};
use serde_json::{Value, json};

const HOMESERVER: &str = "https://matrix.example";
const ROOM: &str = "!abcdefg:matrix.example";
const POST: &str = "$post";
const REPLY: &str = "$reply";

fn matrix() -> Matrix {
    Matrix::new(
        &format!("{HOMESERVER}/"),
        Secret::new("HERALD_MATRIX_TOKEN", "test-token"),
        RouteOptions {
            room: ROOM.to_owned(),
        },
    )
}

fn sent(id: &str) -> Facts {
    Facts {
        message: Some(id.to_owned()),
        ..Facts::default()
    }
}

fn posted() -> Entry {
    Entry::new(announcement().key)
        .landed(Step::Post, sent(POST))
        .landed(Step::Chunk(1), sent(REPLY))
}

fn edit(part: u32) -> Step {
    Step::Edit {
        revision: "r2".to_owned(),
        part,
    }
}

fn request(step: &Step, entry: &Entry) -> Recorded {
    Recorded::of(&matrix().request(step, &announcement(), entry).unwrap())
}

fn room() -> String {
    format!("{HOMESERVER}/_matrix/client/v3/rooms/{}", unreserved(ROOM))
}

fn sending(step: &Step, status: u16, body: Value) -> Sent {
    let entry = posted();
    let replay = Replay::of(Fixture {
        exchanges: vec![Exchange {
            request: request(step, &entry),
            response: Answer {
                status,
                headers: std::collections::BTreeMap::new(),
                body: Some(body),
            },
        }],
    });
    let answer = pollster::block_on(matrix().send(&replay, step, &announcement(), &entry)).unwrap();
    replay.finish().unwrap();
    answer
}

#[test]
fn each_part_is_replaced_by_an_edit_sent_once_per_revision() {
    let entry = posted();
    let key = announcement().key.to_string();
    let post = request(&edit(0), &entry);
    assert_eq!(
        (post.method.as_str(), post.url),
        (
            "PUT",
            format!(
                "{}/send/m.room.message/{}",
                room(),
                unreserved(&format!("{key}.r2.0"))
            )
        )
    );
    let body = post.body.unwrap();
    assert_eq!(
        body["m.relates_to"],
        json!({ "rel_type": "m.replace", "event_id": POST })
    );
    assert!(body["body"].as_str().unwrap().starts_with("* "));
    assert_eq!(body["m.new_content"].get("m.relates_to"), None);
    assert_eq!(body["m.mentions"], json!({}));
    let reply = request(&edit(1), &entry).body.unwrap();
    assert_eq!(reply["m.new_content"]["body"], json!("- the gate"));
    assert_eq!(reply["m.relates_to"]["event_id"], json!(REPLY));
}

#[test]
fn a_reply_no_longer_needed_is_redacted() {
    let key = announcement().key.to_string();
    let redacted = request(&Step::Delete(1), &posted());
    assert_eq!(
        (redacted.method.as_str(), redacted.url, redacted.body),
        (
            "PUT",
            format!(
                "{}/redact/{}/{}",
                room(),
                unreserved(REPLY),
                unreserved(&format!("{key}.redact.1.{}", digest(REPLY)))
            ),
            Some(json!({}))
        )
    );
}

#[test]
fn two_replacements_redact_their_own_events_under_their_own_transactions() {
    let first = request(&Step::Delete(0), &posted());
    let replaced = Entry::new(announcement().key).landed(Step::Post, sent("$reposted"));
    let second = request(&Step::Delete(0), &replaced);
    let transaction = |url: &str| url.rsplit('/').next().unwrap_or_default().to_owned();
    assert!(first.url.contains("/redact/%24post/"), "{}", first.url);
    assert!(
        second.url.contains("/redact/%24reposted/"),
        "{}",
        second.url
    );
    assert_ne!(transaction(&first.url), transaction(&second.url));
}

#[test]
fn an_edit_back_to_an_earlier_revision_goes_under_a_transaction_of_its_own() {
    let at = jiff::Timestamp::from_second(1_000).unwrap();
    let once = posted().attempted(edit(0), at);
    let again = posted().attempted(edit(0), at + jiff::SignedDuration::from_hours(2));
    let retried = once
        .clone()
        .attempted(edit(0), at + jiff::SignedDuration::from_hours(1));
    let retried = request(&edit(0), &retried);
    let (once, again) = (request(&edit(0), &once), request(&edit(0), &again));
    assert!(once.url.ends_with(".r2.0.1000"), "{}", once.url);
    assert_ne!(once.url, again.url);
    assert_eq!(once.url, retried.url);
}

#[test]
fn a_part_never_recorded_or_a_thread_cannot_be_edited_redacted_or_renamed() {
    let entry = Entry::new(announcement().key);
    let threadless = Entry::new(announcement().key).landed(Step::Chunk(1), sent(REPLY));
    for (step, entry) in [
        (edit(0), &entry),
        (edit(1), &threadless),
        (Step::Delete(1), &entry),
        (Step::Rename("r2".to_owned()), &entry),
    ] {
        assert!(
            matrix().request(&step, &announcement(), entry).is_err(),
            "{step:?}"
        );
    }
    let unthreaded = Entry::new(announcement().key)
        .landed(Step::Post, sent(POST))
        .landed(Step::Chunk(2), sent(REPLY));
    assert!(
        matrix()
            .request(&edit(2), &announcement(), &unthreaded)
            .is_err()
    );
}

#[test]
fn an_edit_or_redaction_lands_and_one_of_an_event_gone_lands_too() {
    assert!(matches!(
        sending(&edit(0), 200, json!({ "event_id": "$edit" })),
        Sent::Landed(facts) if facts.message.as_deref() == Some("$edit")
    ));
    assert!(matches!(
        sending(&Step::Delete(1), 200, json!({ "event_id": "$redaction" })),
        Sent::Landed(_)
    ));
    assert_eq!(
        sending(&Step::Delete(1), 404, json!({ "errcode": "M_NOT_FOUND" })),
        Sent::Landed(Facts::default())
    );
}

#[test]
fn a_recorded_event_not_shaped_as_one_is_never_sent_to_and_one_that_is_stays_one_segment() {
    for id in ["..", ".", "%2e%2e", "a/b", "a?b"] {
        let entry = Entry::new(announcement().key)
            .landed(Step::Post, sent(id))
            .landed(Step::Chunk(1), sent(id));
        for step in [edit(0), edit(1), Step::Delete(1)] {
            let refused = matrix()
                .request(&step, &announcement(), &entry)
                .unwrap_err()
                .to_string();
            assert!(
                refused.contains("not an id the platform gives"),
                "{id} {step:?}: {refused}"
            );
        }
    }
    let entry = Entry::new(announcement().key)
        .landed(Step::Post, sent(POST))
        .landed(Step::Chunk(1), sent("$a/../b?c"));
    let redacted = request(&Step::Delete(1), &entry);
    assert!(
        redacted
            .url
            .starts_with(&format!("{}/redact/%24a%2F..%2Fb%3Fc/", room())),
        "{}",
        redacted.url
    );
}
