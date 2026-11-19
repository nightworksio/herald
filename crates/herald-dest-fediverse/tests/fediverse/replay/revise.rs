//! A Mastodon status brought to a new revision: it and each reply edited
//! in place, and each reply no longer needed deleted.

use herald_core::{Entry, Facts, Sent, Step};
use herald_dest_fediverse::{Mastodon, RouteOptions};
use herald_ports::Secret;
use herald_testing::sample::announcement;
use herald_testing::{Answer, Exchange, Fixture, Recorded, Replay};
use serde_json::{Value, json};

const SERVER: &str = "https://mastodon.example";
const POST: &str = "113420000000000001";
const REPLY: &str = "113420000000000002";

fn mastodon() -> Mastodon {
    Mastodon::new(
        &format!("{SERVER}/"),
        Secret::new("HERALD_MASTODON_TOKEN", "test-token"),
        RouteOptions {
            language: Some("en".to_owned()),
            content_warning: Some("Release notes".to_owned()),
            ..RouteOptions::default()
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
    Recorded::of(&mastodon().request(step, &announcement(), entry).unwrap())
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
    let answer =
        pollster::block_on(mastodon().send(&replay, step, &announcement(), &entry)).unwrap();
    replay.finish().unwrap();
    answer
}

#[test]
fn a_status_and_each_reply_are_edited_in_place_and_a_reply_no_longer_needed_deleted() {
    let entry = posted();
    let status = request(&edit(0), &entry);
    assert_eq!(
        (status.method.as_str(), status.url.as_str()),
        ("PUT", format!("{SERVER}/api/v1/statuses/{POST}").as_str())
    );
    let body = status.body.unwrap();
    assert_eq!(
        (&body["language"], &body["spoiler_text"]),
        (&json!("en"), &json!("Release notes"))
    );
    assert_eq!(body.get("visibility"), None);
    assert_eq!(body.get("in_reply_to_id"), None);
    assert!(body["status"].as_str().unwrap().contains("## Added"));
    let reply = request(&edit(1), &entry);
    assert_eq!(reply.url, format!("{SERVER}/api/v1/statuses/{REPLY}"));
    assert_eq!(reply.body.unwrap()["status"], json!("- the gate"));
    let deleted = request(&Step::Delete(1), &entry);
    assert_eq!(
        (deleted.method.as_str(), deleted.url.as_str(), deleted.body),
        (
            "DELETE",
            format!("{SERVER}/api/v1/statuses/{REPLY}").as_str(),
            None
        )
    );
}

#[test]
fn a_status_never_recorded_is_not_edited_and_no_thread_is_renamed() {
    let entry = Entry::new(announcement().key);
    for step in [
        edit(0),
        Step::Delete(1),
        Step::Rename("r2".to_owned()),
        Step::Updates("r2".to_owned()),
    ] {
        assert!(
            mastodon().request(&step, &announcement(), &entry).is_err(),
            "{step:?}"
        );
    }
}

#[test]
fn an_edit_lands_with_its_status_and_a_deletion_of_one_gone_lands_too() {
    assert_eq!(
        sending(
            &edit(0),
            200,
            json!({ "id": POST, "url": "https://mastodon.example/@herald/1" })
        ),
        Sent::Landed(Facts {
            permalink: Some("https://mastodon.example/@herald/1".to_owned()),
            ..sent(POST)
        })
    );
    assert_eq!(
        sending(
            &Step::Delete(1),
            404,
            json!({ "error": "Record not found" })
        ),
        Sent::Landed(Facts::default())
    );
}
