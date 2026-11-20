//! A Slack post brought to a new revision: each part edited in place, and
//! each reply no longer needed deleted.

use herald_core::{Entry, Facts, Sent, Step};
use herald_dest_slack::{Credential, RouteOptions, Slack};
use herald_ports::Secret;
use herald_testing::sample::announcement;
use herald_testing::{Answer, Exchange, Fixture, Recorded, Replay};
use serde_json::{Value, json};

const CHANNEL: &str = "C0123456789";
const POST: &str = "1793448000.000100";
const REPLY: &str = "1793448000.000200";

fn bot() -> Slack {
    Slack::new(
        Credential::Bot(Secret::new("HERALD_SLACK_TOKEN", "xoxb-test")),
        RouteOptions {
            channel: Some(CHANNEL.to_owned()),
            subscribe: None,
        },
        None,
    )
}

fn hook() -> Slack {
    Slack::new(
        Credential::Webhook(Secret::new(
            "HERALD_SLACK_WEBHOOK",
            "https://hooks.slack.com/services/T0/B0/xyz",
        )),
        RouteOptions::default(),
        None,
    )
}

fn sent(ts: &str) -> Facts {
    Facts {
        message: Some(ts.to_owned()),
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

fn request(slack: &Slack, step: &Step, entry: &Entry) -> Recorded {
    Recorded::of(&slack.request(step, &announcement(), entry).unwrap())
}

fn sending(step: &Step, body: Value) -> Sent {
    let slack = bot();
    let entry = posted();
    let replay = Replay::of(Fixture {
        exchanges: vec![Exchange {
            request: request(&slack, step, &entry),
            response: Answer {
                status: 200,
                headers: std::collections::BTreeMap::new(),
                body: Some(body),
            },
        }],
    });
    let answer = pollster::block_on(slack.send(&replay, step, &announcement(), &entry)).unwrap();
    replay.finish().unwrap();
    answer
}

#[test]
fn a_bot_updates_the_post_and_each_reply_and_deletes_a_reply_no_longer_needed() {
    let slack = bot();
    let entry = posted();
    let post = request(&slack, &edit(0), &entry);
    assert_eq!(post.url, "https://slack.com/api/chat.update");
    let body = post.body.unwrap();
    assert_eq!(
        (&body["channel"], &body["ts"]),
        (&json!(CHANNEL), &json!(POST))
    );
    assert!(body.get("blocks").is_some());
    let reply = request(&slack, &edit(1), &entry).body.unwrap();
    assert_eq!(
        (&reply["ts"], &reply["text"]),
        (&json!(REPLY), &json!("- the gate"))
    );
    let deleted = request(&slack, &Step::Delete(1), &entry);
    assert_eq!(deleted.url, "https://slack.com/api/chat.delete");
    assert_eq!(
        deleted.body,
        Some(json!({ "channel": CHANNEL, "ts": REPLY }))
    );
}

#[test]
fn a_webhook_edits_deletes_and_renames_nothing_and_a_part_never_recorded_is_not_edited() {
    let entry = posted();
    for (slack, step) in [
        (hook(), edit(0)),
        (hook(), Step::Delete(1)),
        (bot(), Step::Rename("r2".to_owned())),
        (hook(), Step::Rename("r2".to_owned())),
    ] {
        assert!(
            slack.request(&step, &announcement(), &entry).is_err(),
            "{step:?}"
        );
    }
    let unposted = Entry::new(announcement().key);
    for step in [edit(0), Step::Delete(1)] {
        assert!(
            bot().request(&step, &announcement(), &unposted).is_err(),
            "{step:?}"
        );
    }
}

#[test]
fn an_update_lands_with_its_message_and_a_deletion_of_one_gone_lands_too() {
    assert_eq!(
        sending(
            &edit(0),
            json!({ "ok": true, "channel": CHANNEL, "ts": POST })
        ),
        Sent::Landed(sent(POST))
    );
    for answer in [
        json!({ "ok": true, "channel": CHANNEL, "ts": REPLY }),
        json!({ "ok": false, "error": "message_not_found" }),
    ] {
        assert_eq!(
            sending(&Step::Delete(1), answer),
            Sent::Landed(Facts::default())
        );
    }
    assert!(matches!(
        sending(
            &Step::Delete(1),
            json!({ "ok": false, "error": "cant_delete_message" })
        ),
        Sent::Refused(_)
    ));
}
