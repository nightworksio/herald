//! A Discord post brought to a new revision: each part edited, each chunk
//! no longer needed deleted, and the thread opened from it renamed.

use herald_core::{Delivery, Entry, Facts, Sent, Step};
use herald_dest_discord::{Credential, Discord, RouteOptions};
use herald_ports::Secret;
use herald_testing::sample::{self, announcement};
use herald_testing::{Answer, Exchange, Fixture, Recorded, Replay};
use serde_json::{Value, json};

const CHANNEL: &str = "1566184281406177531";
const POST: &str = "1566200000000000001";
const CHUNK: &str = "1566200000000000002";
const THREAD: &str = "1566200000000000009";
const API: &str = "https://discord.com/api/v10";
const HOOK: &str = "«secret:HERALD_DISCORD_WEBHOOK»";

fn bot(forum: bool) -> Discord {
    Discord::new(
        Credential::Bot(Secret::new("HERALD_DISCORD_TOKEN", "test-token")),
        RouteOptions {
            channel: Some(CHANNEL.to_owned()),
            forum,
            subscribe: None,
            viewers: Vec::new(),
        },
        None,
    )
}

fn webhook() -> Discord {
    Discord::new(
        Credential::Webhook(Secret::new(
            "HERALD_DISCORD_WEBHOOK",
            "https://discord.com/api/webhooks/1/hook",
        )),
        RouteOptions::default(),
        None,
    )
}

fn delivery() -> Delivery {
    sample::delivery("1566190000000000001")
}

fn sent(message: &str) -> Facts {
    Facts {
        message: Some(message.to_owned()),
        ..Facts::default()
    }
}

fn edit(part: u32) -> Step {
    Step::Edit {
        revision: "r2".to_owned(),
        part,
    }
}

/// An entry whose post and first chunk landed, the chunk in `thread`
/// where it names one.
fn posted(thread: Option<&str>) -> Entry {
    let entry = Entry::new(announcement().key)
        .landed(Step::Post, sent(POST))
        .landed(Step::Chunk(1), sent(CHUNK));
    match thread {
        Some(thread) => entry.landed(
            Step::OpenThread,
            Facts {
                thread: Some(thread.to_owned()),
                ..Facts::default()
            },
        ),
        None => entry,
    }
}

fn request(discord: &Discord, step: &Step, entry: &Entry) -> Recorded {
    Recorded::of(
        &discord
            .request(step, &announcement(), &delivery(), entry)
            .unwrap(),
    )
}

fn sending(discord: &Discord, step: &Step, entry: &Entry, status: u16, body: Value) -> Sent {
    let replay = Replay::of(Fixture {
        exchanges: vec![Exchange {
            request: request(discord, step, entry),
            response: Answer {
                status,
                headers: std::collections::BTreeMap::new(),
                body: Some(body),
            },
        }],
    });
    let answer =
        pollster::block_on(discord.send(&replay, step, &announcement(), &delivery(), entry))
            .unwrap();
    replay.finish().unwrap();
    answer
}

#[test]
fn a_bot_edits_the_post_and_each_chunk_where_it_was_posted_and_renames_its_thread() {
    let discord = bot(false);
    let entry = posted(Some(THREAD));
    let post = request(&discord, &edit(0), &entry);
    assert_eq!(
        (post.method.as_str(), post.url.as_str()),
        (
            "PATCH",
            format!("{API}/channels/{CHANNEL}/messages/{POST}").as_str()
        )
    );
    let body = post.body.unwrap();
    assert_eq!(body["embeds"][0]["description"], json!("## Added"));
    assert_eq!(body.get("nonce"), None);
    let chunk = request(&discord, &edit(1), &entry);
    assert_eq!(
        chunk.url,
        format!("{API}/channels/{THREAD}/messages/{CHUNK}")
    );
    assert_eq!(chunk.body.unwrap()["content"], json!("- the gate"));
    let deleted = request(&discord, &Step::Delete(1), &entry);
    assert_eq!(
        (deleted.method.as_str(), deleted.url.as_str(), deleted.body),
        (
            "DELETE",
            format!("{API}/channels/{THREAD}/messages/{CHUNK}").as_str(),
            None
        )
    );
    let renamed = request(&discord, &Step::Rename("r2".to_owned()), &entry);
    assert_eq!(
        (renamed.method.as_str(), renamed.url.as_str()),
        ("PATCH", format!("{API}/channels/{THREAD}").as_str())
    );
    assert_eq!(
        renamed.body,
        Some(json!({ "name": "php-mutation-gate v0.1.0" }))
    );
}

#[test]
fn a_bot_edits_a_forum_post_in_its_own_thread() {
    let discord = bot(true);
    let entry = Entry::new(announcement().key).landed(
        Step::Post,
        Facts {
            message: Some(POST.to_owned()),
            thread: Some(THREAD.to_owned()),
            permalink: None,
        },
    );
    assert_eq!(
        request(&discord, &edit(0), &entry).url,
        format!("{API}/channels/{THREAD}/messages/{POST}")
    );
}

#[test]
fn a_webhook_edits_and_deletes_its_own_messages_and_renames_nothing() {
    let discord = webhook();
    let entry = posted(None);
    let post = request(&discord, &edit(0), &entry);
    assert_eq!(
        (post.method.as_str(), post.url.as_str()),
        (
            "PATCH",
            format!("{HOOK}/messages/{POST}?with_components=true").as_str()
        )
    );
    assert_eq!(
        request(&discord, &edit(1), &entry).url,
        format!("{HOOK}/messages/{CHUNK}")
    );
    let threaded = Entry::new(announcement().key)
        .landed(
            Step::Post,
            Facts {
                message: Some(POST.to_owned()),
                thread: Some(THREAD.to_owned()),
                permalink: None,
            },
        )
        .landed(Step::Chunk(1), sent(CHUNK));
    let deleted = request(&discord, &Step::Delete(1), &threaded);
    assert_eq!(
        (deleted.method.as_str(), deleted.url.as_str()),
        (
            "DELETE",
            format!("{HOOK}/messages/{CHUNK}?thread_id={THREAD}").as_str()
        )
    );
    assert_eq!(
        request(&discord, &edit(0), &threaded).url,
        format!("{HOOK}/messages/{POST}?thread_id={THREAD}&with_components=true")
    );
    assert!(matches!(
        pollster::block_on(discord.send(
            &Replay::of(Fixture::default()),
            &Step::Rename("r2".to_owned()),
            &announcement(),
            &delivery(),
            &threaded,
        )),
        Ok(Sent::Refused(reason)) if reason.contains("rename a thread")
    ));
}

#[test]
fn a_part_or_thread_never_recorded_cannot_be_edited_deleted_or_renamed() {
    let entry = Entry::new(announcement().key);
    for (discord, step) in [
        (bot(false), edit(0)),
        (bot(false), Step::Delete(1)),
        (bot(false), Step::Rename("r2".to_owned())),
        (webhook(), edit(1)),
        (webhook(), Step::Delete(1)),
    ] {
        assert!(
            discord
                .request(&step, &announcement(), &delivery(), &entry)
                .is_err(),
            "{step:?}"
        );
    }
}

#[test]
fn an_edit_lands_with_its_message_and_a_deletion_of_one_gone_lands_too() {
    let discord = bot(false);
    let entry = posted(Some(THREAD));
    assert_eq!(
        sending(&discord, &edit(0), &entry, 200, json!({ "id": POST })),
        Sent::Landed(sent(POST))
    );
    for status in [204, 404] {
        assert_eq!(
            sending(&discord, &Step::Delete(1), &entry, status, json!("")),
            Sent::Landed(Facts::default()),
            "{status}"
        );
    }
    assert_eq!(
        sending(
            &discord,
            &Step::Rename("r2".to_owned()),
            &entry,
            200,
            json!({ "id": THREAD })
        ),
        Sent::Landed(Facts::default())
    );
    assert!(matches!(
        sending(
            &discord,
            &Step::Delete(1),
            &entry,
            403,
            json!({ "code": 50013, "message": "Missing Permissions" })
        ),
        Sent::Refused(_)
    ));
}

#[test]
fn a_bot_s_chunk_posted_again_by_a_later_revision_carries_a_nonce_of_its_own() {
    let discord = bot(false);
    let nonce =
        |entry: &Entry| request(&discord, &Step::Chunk(1), entry).body.unwrap()["nonce"].clone();
    let posted = Entry::new(announcement().key).landed(Step::Post, sent(POST));
    let revised = posted.clone().revising(herald_core::Revision::of(
        Some(herald_core::Signal::Release),
        ["notes"],
    ));
    assert_eq!(nonce(&posted), json!("5b96fb84465ead32a076c69d8"));
    assert_ne!(nonce(&revised), nonce(&posted));
    assert_eq!(nonce(&revised).as_str().map(str::len), Some(25));
}

#[test]
fn a_recorded_id_that_is_no_snowflake_is_never_sent_to() {
    for id in ["..", ".", "%2e%2e", "1/2", "1?x=2"] {
        let message = Entry::new(announcement().key)
            .landed(Step::Post, sent(id))
            .landed(Step::Chunk(1), sent(id));
        let threaded = Entry::new(announcement().key)
            .landed(Step::Post, sent(POST))
            .landed(Step::Chunk(1), sent(CHUNK))
            .landed(
                Step::OpenThread,
                Facts {
                    thread: Some(id.to_owned()),
                    ..Facts::default()
                },
            );
        for (discord, step, entry) in [
            (bot(false), edit(0), &message),
            (bot(false), Step::Delete(1), &message),
            (bot(false), Step::Pin, &message),
            (webhook(), edit(0), &message),
            (bot(false), Step::Rename("r2".to_owned()), &threaded),
            (bot(false), Step::Delete(1), &threaded),
            (webhook(), edit(1), &threaded),
            (bot(false), Step::Chunk(2), &threaded),
        ] {
            let refused = discord
                .request(&step, &announcement(), &delivery(), entry)
                .unwrap_err()
                .to_string();
            assert!(
                refused.contains("not an id the platform gives"),
                "{id} {step:?}: {refused}"
            );
        }
    }
}
