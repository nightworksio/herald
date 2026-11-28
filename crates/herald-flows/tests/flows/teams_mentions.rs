//! A Microsoft Teams bot's route mentions the members subscribed, by their
//! Microsoft Entra object id, and offers the button that subscribes.

use herald_config::{Config, Uses, load};
use herald_core::{DestinationName, SourceName, Step, Subscription};
use herald_flows::{Outcome, Ports, announce, prepare};
use herald_ports::Subscriptions;
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};
use serde_json::{Value, json};

use crate::harness::*;

const APP: &str = "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9";
const MEMBER: &str = "7faf8ab2-3d56-4244-b585-20c8a42ed2b8";

fn teams(route: &str) -> Result<Config, String> {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.chat]
kind = "teams"
app_id = "{APP}"
app_password = {{ secret = "HERALD_TEAMS_APP_PASSWORD" }}
login = "https://signin.example"
service = "https://smba.example/teams/"
[destinations.chat.interactions]
via = "http"
[sources.gate]
kind = "github"
repository = "o/r"
[[routes]]
id = "carded"
sources = ["gate"]
destination = "chat"
template = "t"
conversation = "19:abc@thread.tacv2"
{route}
[templates.t]
title = "{{{{ event.title }}}}"
body = "{{{{ event.body }}}}"
"#
    );
    load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .map_err(|errors| format!("{errors:?}"))
}

fn subscribed(store: &Failing, subscriber: &str) {
    let subscription = Subscription {
        destination: DestinationName::new("chat"),
        project: "php-mutation-gate".to_owned(),
        subscriber: subscriber.to_owned(),
        inbox: None,
        pending: None,
    };
    pollster::block_on(store.subscribe(&subscription)).unwrap();
}

fn announced(config: &Config, store: &Failing, script: &Script) -> Outcome {
    let secrets = FakeSecrets::default().with("HERALD_TEAMS_APP_PASSWORD", "app-secret");
    let prepared = pollster::block_on(prepare(config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let ports = Ports {
        store,
        clock: &FakeClock::at(NOON),
        signer: &FakeSigner,
        budget: herald_flows::Budget::UNBOUNDED,
    };
    let announced = pollster::block_on(announce(
        config,
        &connected,
        &ports,
        &SourceName::new("gate"),
        &event("v1"),
        "r1",
    ));
    outcome(&announced).clone()
}

fn signing_in() -> Script {
    Script::answering(&[
        ("", 200, r#"{"access_token":"t","expires_in":3600}"#),
        ("", 201, r#"{"id":"1481567603816"}"#),
        ("", 201, r#"{"id":"1481567603817"}"#),
    ])
}

/// The card content the request `body` posts.
fn content(body: &[u8]) -> Value {
    let posted: Value = serde_json::from_slice(body).unwrap();
    posted["attachments"][0]["content"].clone()
}

#[test]
fn a_bot_s_route_mentions_its_subscribers_by_entra_object_id_and_offers_its_button() {
    let config = teams("mentions = { subscribers = true }\nsubscribe = {}").unwrap();
    let store = healthy();
    subscribed(&store, MEMBER);
    subscribed(&store, "@ana:matrix.example");
    let script = signing_in();
    assert_eq!(
        announced(&config, &store, &script),
        Outcome::Sent {
            steps: vec![Step::Post]
        }
    );
    assert_eq!(
        script.seen()[1],
        "POST https://smba.example/teams/v3/conversations/19%3Aabc%40thread.tacv2/activities"
    );
    let card = content(&script.bodies()[1]);
    assert_eq!(
        card["msteams"]["entities"],
        json!([{
            "type": "mention",
            "text": "<at>subscriber 1</at>",
            "mentioned": { "id": MEMBER, "name": "subscriber 1" },
        }])
    );
    let body = card["body"].as_array().unwrap();
    assert_eq!(body.last().unwrap()["text"], "<at>subscriber 1</at>");
    let button = card["actions"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(
        button,
        json!({
            "type": "Action.Execute",
            "title": "Subscribe",
            "verb": "herald_subscribe",
            "data": { "project": "php-mutation-gate" },
        })
    );
    assert_eq!(card["version"], "1.5");
}

#[test]
fn a_bot_s_route_that_mentions_no_subscribers_writes_no_mention_and_no_button() {
    let config = teams("").unwrap();
    let store = healthy();
    subscribed(&store, MEMBER);
    let script = signing_in();
    announced(&config, &store, &script);
    let card = content(&script.bodies()[1]);
    assert_eq!(card.get("msteams"), None);
    assert_eq!(card["version"], "1.0");
    let quiet = teams("mentions = { subscribers = true }\nsubscribe = { button = false }").unwrap();
    let store = healthy();
    subscribed(&store, MEMBER);
    let script = signing_in();
    announced(&quiet, &store, &script);
    let card = content(&script.bodies()[1]);
    assert_eq!(card["msteams"]["entities"][0]["mentioned"]["id"], MEMBER);
    assert!(
        card["actions"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|action| action["type"] != "Action.Execute")
    );
    assert_eq!(card["version"], "1.0");
}

#[test]
fn subscribers_past_the_card_s_room_are_mentioned_in_a_card_after_it() {
    let config = teams("mentions = { subscribers = true }").unwrap();
    let store = healthy();
    for index in 0..40 {
        subscribed(&store, &format!("7faf8ab2-3d56-4244-b585-{index:012x}"));
    }
    let script = signing_in();
    assert_eq!(
        announced(&config, &store, &script),
        Outcome::Sent {
            steps: vec![Step::Post, Step::Chunk(1)]
        }
    );
    let bodies = script.bodies();
    let first = content(&bodies[1])["msteams"]["entities"]
        .as_array()
        .unwrap()
        .len();
    let after = content(&bodies[2]);
    let rest = after["msteams"]["entities"].as_array().unwrap().len();
    assert_eq!((first + rest, first > 0, rest > 0), (40, true, true));
    assert_eq!(
        after["body"][0]["text"]
            .as_str()
            .unwrap()
            .split(' ')
            .filter(|word| word.starts_with("<at>"))
            .count(),
        rest
    );
}

#[test]
fn a_route_offers_the_button_only_where_it_mentions_subscribers_and_the_bot_answers_subscribe() {
    let unmentioned = teams("subscribe = {}").unwrap_err();
    assert!(
        unmentioned.contains("add mentions = { subscribers = true } to the route"),
        "{unmentioned}"
    );
    let text = format!(
        r#"version = 1
[destinations.chat]
kind = "teams"
app_id = "{APP}"
app_password = {{ secret = "HERALD_TEAMS_APP_PASSWORD" }}
[sources.gate]
kind = "github"
repository = "o/r"
[[routes]]
id = "carded"
sources = ["gate"]
destination = "chat"
template = "t"
conversation = "19:abc@thread.tacv2"
mentions = {{ subscribers = true }}
subscribe = {{}}
[templates.t]
title = "{{{{ event.title }}}}"
"#
    );
    let unanswered = format!(
        "{:?}",
        load(
            "herald.toml",
            &text,
            &|_| Err(String::new()),
            &crate::harness::kinds()
        )
        .unwrap_err()
    );
    assert!(
        unanswered.contains("add \\\"subscribe\\\" to [destinations.chat.interactions] commands"),
        "{unanswered}"
    );
}
