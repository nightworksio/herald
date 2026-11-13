use herald_config::{Config, Uses, load};
use herald_core::{
    Chore, DestinationName, Identity, Key, Marker, RouteId, Schedule, SourceName, Step,
    Subscription,
};
use herald_flows::{Chored, Outcome, Ports, Reached, announce, prepare};
use herald_ports::{Markers as _, Schedules, Subscriptions};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner};

use crate::harness::*;

const BOT: &str = r#"token = { secret = "HERALD_SLACK_TOKEN" }"#;
const HOOK: &str = r#"webhook = { secret = "HERALD_SLACK_WEBHOOK" }"#;

fn slack(credential: &str, route: &str) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.team]
kind = "slack"
{credential}
[sources.gate]
kind = "github"
repository = "o/r"
[[routes]]
id = "slacked"
sources = ["gate"]
destination = "team"
template = "t"
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
    .unwrap()
}

fn secrets() -> FakeSecrets {
    FakeSecrets::default()
        .with("HERALD_SLACK_TOKEN", "xoxb-t0ken")
        .with(
            "HERALD_SLACK_WEBHOOK",
            "https://hooks.slack.com/services/T/B/x",
        )
}

fn announced(config: &Config, store: &Failing, script: &Script) -> Outcome {
    let prepared =
        pollster::block_on(prepare(config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let ports = Ports {
        store,
        clock: &FakeClock::at(NOON),
        signer: &FakeSigner,
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

#[test]
fn a_slack_bot_mentions_the_subscribers_herald_keeps_and_records_its_permalink() {
    let store = healthy();
    let subscription = Subscription {
        destination: DestinationName::new("team"),
        project: "php-mutation-gate".to_owned(),
        subscriber: "U0123ABCD".to_owned(),
        inbox: None,
        pending: None,
    };
    pollster::block_on(store.subscribe(&subscription)).unwrap();
    let script = Script::answering(&[
        (
            "/api/chat.postMessage",
            200,
            r#"{"ok":true,"channel":"C0123456789","ts":"1793448000.000100"}"#,
        ),
        (
            "/api/chat.getPermalink?channel=C0123456789&message_ts=1793448000.000100",
            200,
            r#"{"ok":true,"permalink":"https://team.slack.com/archives/C0123456789/p1793448000000100"}"#,
        ),
    ]);
    let config = slack(
        BOT,
        "channel = \"C0123456789\"\nmentions = { subscribers = true }",
    );
    assert_eq!(
        announced(&config, &store, &script),
        Outcome::Sent {
            steps: vec![Step::Post]
        }
    );
    assert_eq!(
        script.seen()[0],
        "POST https://slack.com/api/chat.postMessage"
    );
    let content: serde_json::Value = serde_json::from_slice(&script.bodies()[0]).unwrap();
    assert!(
        content["text"]
            .as_str()
            .unwrap()
            .ends_with("\n\n<@U0123ABCD>"),
        "{content}"
    );
    assert_eq!(content["channel"], "C0123456789");
}

#[test]
fn a_slack_webhook_posts_where_it_was_made() {
    let store = healthy();
    let script = Script::answering(&[("«secret:HERALD_SLACK_WEBHOOK»", 200, "ok")]);
    assert_eq!(
        announced(&slack(HOOK, ""), &store, &script),
        Outcome::Sent {
            steps: vec![Step::Post]
        }
    );
    assert_eq!(script.seen(), ["POST «secret:HERALD_SLACK_WEBHOOK»"]);
}

#[test]
fn a_slack_route_keeps_no_thread_chores_and_its_channel_lists_no_threads() {
    let config = slack(BOT, "channel = \"C0123456789\"");
    let store = healthy();
    let thread = Schedule {
        key: Key::derive(&RouteId::new("slacked"), &Identity::new("1")),
        route: RouteId::new("slacked"),
        thread: "1".to_owned(),
        chore: Chore::Archive,
        due: NOON,
    };
    pollster::block_on(store.put_schedules(&[thread], &[])).unwrap();
    let script = Script::default();
    let prepared =
        pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let kept = housekept(&connected, &store, &herald_core::Retention::default())
        .unwrap()
        .kept;
    assert_eq!(
        kept[0].outcome,
        Chored::Dropped("Slack cannot archive or lock a thread".to_owned())
    );
    assert_eq!(script.seen(), Vec::<String>::new());
    let reached = Reached::new(herald_dest_slack::Slack::new(
        herald_dest_slack::Credential::Bot(herald_ports::Secret::new(
            "HERALD_SLACK_TOKEN",
            "xoxb-t0ken",
        )),
        herald_dest_slack::RouteOptions {
            channel: Some("C0123456789".to_owned()),
            subscribe: None,
        },
        None,
    ));
    assert_eq!(reached.api(), "https://slack.com/api");
    assert_eq!(reached.profile(), herald_dest_slack::profile(true));
    let listed = pollster::block_on(reached.read(&script).threads(&Marker::of(&[1]))).unwrap();
    assert_eq!(listed, []);
    let key = Key::derive(&RouteId::new("slacked"), &Identity::new("1"));
    let read = pollster::block_on(reached.read(&script).entry(&key, &Marker::of(&[1])));
    let Err(herald_ports::MarkersError::Unreadable { reason }) = read else {
        panic!("{read:?}");
    };
    assert!(reason.contains("nothing scripted"), "{reason}");
}
