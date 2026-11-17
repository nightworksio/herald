use herald_core::{DestinationName, Signal, Subscription};
use herald_ports::Subscriptions;
use herald_testing::FakeSecrets;

use super::*;

const POSTED_TO_SLACK: (&str, u16, &str) = (
    "/api/chat.postMessage",
    200,
    r#"{"ok":true,"channel":"C0123456789","ts":"1793448000.000100"}"#,
);
const PERMALINK: (&str, u16, &str) = (
    "/api/chat.getPermalink?channel=C0123456789&message_ts=1793448000.000100",
    200,
    r#"{"ok":true,"permalink":"https://team.slack.com/archives/C0123456789/p1793448000000100"}"#,
);

fn slack() -> Config {
    slack_as(
        "token = { secret = \"HERALD_SLACK_TOKEN\" }",
        "channel = \"C0123456789\"\nmentions = { subscribers = true }",
        "",
    )
}

/// A Slack destination reached with `credential`, its route given `route`
/// and its group `group` beside its key, lead and `until`.
fn slack_as(credential: &str, route: &str, group: &str) -> Config {
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
id = "train"
sources = ["gate"]
destination = "team"
template = "t"
{route}
group = {{ key = "{{{{ event.attributes.version }}}}", lead = ["core"], until = "lead"{group} }}
[templates.t]
title = "{{{{ event.title }}}}"
body = "{{% for member in events %}}{{{{ member.project }}}} {{% endfor %}}"
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

fn subscribe(store: &Failing, project: &str, subscriber: &str) {
    let subscription = Subscription {
        destination: DestinationName::new("team"),
        project: project.to_owned(),
        subscriber: subscriber.to_owned(),
        inbox: None,
        pending: None,
    };
    pollster::block_on(store.subscribe(&subscription)).unwrap();
}

fn slacked(config: &Config, store: &Failing, script: &Script, event: &Event) -> Outcome {
    let secrets = FakeSecrets::default()
        .with("HERALD_SLACK_TOKEN", "xoxb-t0ken")
        .with(
            "HERALD_SLACK_WEBHOOK",
            "https://hooks.slack.com/services/T/B/x",
        );
    let prepared = pollster::block_on(prepare(config, &secrets, &FakeSigner, Uses::Polls)).unwrap();
    let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
    let ports = Ports {
        store,
        clock: &FakeClock::at(NOON),
        signer: &FakeSigner,
        budget: Budget::UNBOUNDED,
    };
    let announced = pollster::block_on(announce(
        config,
        &connected,
        &ports,
        &SourceName::new("gate"),
        event,
        "r1",
    ));
    outcome(&announced).clone()
}

#[test]
fn a_group_mentions_the_subscribers_of_every_member_s_project_each_once() {
    let (config, store) = (slack(), healthy());
    subscribe(&store, "core", "U0000CORE");
    subscribe(&store, "core", "U0000BOTH");
    subscribe(&store, "cli", "U0000BOTH");
    subscribe(&store, "cli", "U00000CLI");
    subscribe(&store, "web", "U00000WEB");
    let script = Script::answering(&[POSTED_TO_SLACK, PERMALINK]);
    assert_eq!(
        slacked(&config, &store, &script, &release("cli", 2)),
        gathered(30)
    );
    assert_eq!(
        slacked(&config, &store, &script, &release("core", 1)),
        posted()
    );
    let content: serde_json::Value = serde_json::from_slice(&script.bodies()[0]).unwrap();
    let text = content["text"].as_str().unwrap();
    assert!(text.contains("core cli"), "{text}");
    assert!(
        text.ends_with("\n\n<@U0000BOTH> <@U0000CORE> <@U00000CLI>"),
        "{text}"
    );
}

#[test]
fn a_destination_that_edits_no_post_posts_a_late_member_and_a_late_release_anew() {
    let hooked = (
        "webhook = { secret = \"HERALD_SLACK_WEBHOOK\" }",
        ", late = \"edit\"",
    );
    let (config, store) = (slack_as(hooked.0, "", hooked.1), healthy());
    let landed = ("«secret:HERALD_SLACK_WEBHOOK»", 200, "ok");
    let script = Script::answering(&[landed, landed, landed]);
    slacked(
        &config,
        &store,
        &script,
        &member("cli", 2, Signal::Tag, "tagged"),
    );
    assert_eq!(
        slacked(&config, &store, &script, &release("core", 1)),
        posted()
    );
    assert_eq!(
        slacked(&config, &store, &script, &release("web", 3)),
        posted()
    );
    let alone: serde_json::Value = serde_json::from_slice(&script.bodies()[1]).unwrap();
    assert!(alone["text"].as_str().unwrap().contains("web"), "{alone}");
    assert_eq!(
        slacked(
            &config,
            &store,
            &script,
            &member("cli", 2, Signal::Release, "released")
        ),
        posted()
    );
    assert_eq!(script.seen().len(), 3);
}
