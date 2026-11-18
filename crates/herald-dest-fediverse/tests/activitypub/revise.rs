//! An ActivityPub Note brought to a new revision: its `Update` delivered
//! to each follower its `Create` reached, and its parts carried in it.

use std::sync::Mutex;

use herald_core::{Entry, Facts, Revision, Sent, Signal, Step};
use herald_dest_fediverse::{ActivityPub, ActorRouteOptions, Audience};
use herald_ports::{Http, HttpError, Request, Response, Secret};
use herald_testing::FakeSigner;
use herald_testing::sample::announcement;
use jiff::Timestamp;
use serde_json::{Value, json};

const POST: &str = "113420000000000001";
const REPLY: &str = "113420000000000002";
const INBOX: &str = "https://social.example/inbox";

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

/// An `Http` that answers every request accepted, and keeps what it
/// was sent.
#[derive(Default)]
struct Accepting(Mutex<Vec<Request>>);

impl Http for Accepting {
    fn send(&self, request: Request) -> impl Future<Output = Result<Response, HttpError>> + Send {
        self.0.lock().unwrap().push(request);
        std::future::ready(Ok(Response {
            status: 202,
            headers: Vec::new(),
            body: Vec::new(),
        }))
    }
}

fn actor() -> ActivityPub {
    ActivityPub::new(
        "herald.example",
        "",
        Secret::new("HERALD_ACTIVITYPUB_KEY", "the-key"),
        ActorRouteOptions {
            actor: "php-mutation-gate".to_owned(),
            content_warning: None,
            visibility: Audience::Public,
            language: None,
        },
    )
}

fn delivered(entry: &Entry) -> Value {
    let http = Accepting::default();
    let now: Timestamp = "2026-11-03T08:00:00Z".parse().unwrap();
    let answer = pollster::block_on(actor().send(
        &http,
        &FakeSigner,
        now,
        &Step::Deliver(INBOX.to_owned()),
        &announcement(),
        entry,
    ))
    .unwrap();
    assert_eq!(answer, Sent::Landed(Facts::default()));
    let sent = http.0.lock().unwrap();
    serde_json::from_slice(&sent[0].body).unwrap()
}

#[test]
fn a_follower_takes_the_create_until_the_deliveries_landed_and_then_each_revision_s_update() {
    let revision = Revision::of(Some(Signal::Release), ["notes"]);
    let entry = Entry::new(announcement().key).revising(revision.clone());
    assert_eq!(delivered(&entry)["type"], json!("Create"));
    let delivered_once = entry.landed(Step::Deliveries, Facts::default());
    let update = delivered(&delivered_once);
    let note = format!(
        "https://herald.example/users/php-mutation-gate/notes/{}",
        announcement().key
    );
    assert_eq!(update["type"], json!("Update"));
    assert_eq!(
        update["id"],
        json!(format!("{note}/activity/{}", revision.digest))
    );
    assert_eq!(update["object"]["id"], json!(note));
    assert_eq!(update["object"]["updated"], json!("2026-11-03T08:00:00Z"));
    assert_eq!(update["published"], json!("2026-11-03T08:00:00Z"));
}

#[test]
fn a_follower_the_note_reached_takes_its_delete_with_a_tombstone_once_it_is_withdrawn() {
    let revision = Revision::of(Some(Signal::Release), ["notes"]);
    let withdrawing = Entry::new(announcement().key)
        .revising(revision)
        .landed(Step::Deliveries, Facts::default())
        .wanting(&[Step::Delete(0), Step::Deletes]);
    let deleted = delivered(&withdrawing);
    let note = format!(
        "https://herald.example/users/php-mutation-gate/notes/{}",
        announcement().key
    );
    assert_eq!(deleted["type"], json!("Delete"));
    assert_eq!(deleted["id"], json!(format!("{note}/activity/deleted")));
    assert_eq!(
        deleted["actor"],
        json!("https://herald.example/users/php-mutation-gate")
    );
    assert_eq!(deleted["object"]["id"], json!(note));
    assert_eq!(deleted["object"]["type"], json!("Tombstone"));
    assert_eq!(deleted["object"]["deleted"], json!("2026-11-03T08:00:00Z"));
    assert_eq!(deleted["published"], json!("2026-11-03T08:00:00Z"));
    assert_eq!(
        deleted["to"],
        json!(["https://www.w3.org/ns/activitystreams#Public"])
    );
    let unrevised = Entry::new(announcement().key).wanting(&[Step::Delete(0), Step::Deletes]);
    assert_eq!(delivered(&unrevised)["type"], json!("Delete"));
    let editing = Entry::new(announcement().key).wanting(&[Step::Post, Step::Deliveries]);
    assert_eq!(delivered(&editing)["type"], json!("Create"));
}

#[test]
fn a_note_carries_every_part_so_its_edits_and_deletions_ask_nothing() {
    let http = Accepting::default();
    let now: Timestamp = "2026-11-03T08:00:00Z".parse().unwrap();
    for step in [edit(0), edit(1), Step::Delete(1)] {
        assert_eq!(
            pollster::block_on(actor().send(
                &http,
                &FakeSigner,
                now,
                &step,
                &announcement(),
                &posted()
            ))
            .unwrap(),
            Sent::Landed(Facts::default()),
            "{step:?}"
        );
    }
    for step in [
        Step::Rename("r2".to_owned()),
        Step::Updates("r2".to_owned()),
        Step::Deletes,
    ] {
        assert!(matches!(
            pollster::block_on(actor().send(
                &http,
                &FakeSigner,
                now,
                &step,
                &announcement(),
                &posted()
            ))
            .unwrap(),
            Sent::Refused(_)
        ));
    }
    assert!(http.0.lock().unwrap().is_empty());
}
