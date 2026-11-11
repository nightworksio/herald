use herald_core::{ReadBack, Waits, Wording};
use herald_kinds::{Connect, Effect};
use herald_testing::FakeSecrets;
use serde_json::{Value, json};

use super::*;
use crate::domain::Domain;
use crate::interact::Replying as Reply;

const CHANNEL: &str = "C0123456789";

fn options(value: &Value) -> DestinationOptions {
    serde_json::from_value(value.clone()).unwrap()
}

fn bot(interactions: &Value) -> DestinationOptions {
    options(&json!({
        "token": { "secret": "HERALD_SLACK_TOKEN" },
        "interactions": interactions,
    }))
}

fn webhook(interactions: &Value) -> DestinationOptions {
    options(&json!({
        "webhook": { "secret": "HERALD_SLACK_WEBHOOK" },
        "interactions": interactions,
    }))
}

fn route(value: &Value) -> RouteOptions {
    serde_json::from_value(value.clone()).unwrap()
}

fn socket() -> Value {
    json!({
        "via": "socket",
        "app": { "secret": "HERALD_SLACK_APP" },
        "domains": ["proxy.example"],
    })
}

fn posted() -> Value {
    json!({ "signing": { "secret": "HERALD_SLACK_SIGNING" } })
}

fn placing(subscribers: bool) -> Placing<'static> {
    Placing {
        destination: "slack",
        subscribers,
    }
}

fn nothing(_: &str) -> Result<String, String> {
    Err("no file".to_owned())
}

fn secret() -> Secret {
    Secret::new("HERALD_SLACK_TOKEN", "xoxb-1")
}

fn messages(faults: &[Fault]) -> Vec<(Option<&str>, &str)> {
    faults
        .iter()
        .map(|fault| (fault.key.as_deref(), fault.message.as_str()))
        .collect()
}

fn names<'a>(refs: &[&'a SecretRef]) -> Vec<&'a str> {
    refs.iter().map(|named| named.secret.as_str()).collect()
}

#[test]
fn a_destination_reads_as_a_slack_one_through_its_bot_or_webhook() {
    let written = options(&json!({
        "token": { "secret": "HERALD_SLACK_TOKEN" },
        "symbols": { "bullet": "*" },
        "waits": { "outage": "7s" },
    }));
    assert_eq!(written.kind(), "slack");
    assert_eq!(written.symbols_written(), written.symbols);
    assert_ne!(written.symbols_written(), Some(Symbols::default()));
    assert_eq!(written.waits_written(), written.waits);
    assert_ne!(written.waits_written(), Some(WaitOptions::default()));
    assert_eq!(
        written.credential().map(|named| named.secret.as_str()),
        Some("HERALD_SLACK_TOKEN")
    );
    assert_eq!(names(&written.secret_refs()), ["HERALD_SLACK_TOKEN"]);
    assert_eq!(written.answering(), None);
    assert_eq!(written.carried(), None);

    let hooked = webhook(&json!({
        "signing": { "secret": "HERALD_SLACK_SIGNING" },
        "app": { "secret": "HERALD_SLACK_APP" },
    }));
    assert_eq!(
        hooked.credential().map(|named| named.secret.as_str()),
        Some("HERALD_SLACK_WEBHOOK")
    );
    assert_eq!(
        names(&hooked.secret_refs()),
        [
            "HERALD_SLACK_WEBHOOK",
            "HERALD_SLACK_SIGNING",
            "HERALD_SLACK_APP"
        ]
    );
    assert_eq!(
        hooked.answering(),
        hooked.interactions.as_ref().map(Interactions::answering)
    );
}

#[test]
fn commands_are_posted_unless_socket_mode_carries_them_under_its_domains() {
    let socketed = bot(&socket());
    assert_eq!(
        socketed.session_domains(),
        [Domain::Slack.host(), "proxy.example"]
    );
    assert_eq!(
        socketed.carried(),
        Some(Carried {
            posted: false,
            what: "commands Slack posts",
            session: "Socket Mode with via = \"socket\"",
        })
    );
    let answering = bot(&posted());
    assert_eq!(answering.session_domains(), Vec::<&str>::new());
    assert_eq!(
        answering.carried().map(|carried| carried.posted),
        Some(true)
    );
}

#[test]
fn a_route_reaches_and_profiles_slack_as_its_bot_or_webhook() {
    let channelled = route(&json!({ "channel": CHANNEL }));
    let placed = Placed::new(channelled.clone());
    let answering = bot(&posted());
    assert_eq!(
        answering.profile(&placed),
        Some((slack::profile(true), Some(slack::profile(true))))
    );
    assert_eq!(
        webhook(&posted()).profile(&placed),
        Some((slack::profile(false), Some(slack::profile(true))))
    );
    let reached = answering.reach(&placed, "", secret());
    assert_eq!(
        reached,
        Some(Reached::new(
            Slack::new(Credential::Bot(secret()), channelled.clone(), None)
                .on(Domain::default())
                .in_team(None)
                .reading_back(ReadBack::DEFAULT)
                .waiting(Waits::DEFAULT)
                .worded(Wording::default())
        ))
    );
    assert_eq!(
        reached.map(|reached| reached.api().to_owned()),
        Some("https://slack.com/api".to_owned())
    );
    assert_eq!(
        webhook(&posted()).reach(&placed, "", secret()),
        Some(Reached::new(Slack::new(
            Credential::Webhook(secret()),
            channelled,
            None
        )))
    );
    assert_eq!(route(&json!({})).place(), None);
    assert_eq!(route(&json!({ "channel": CHANNEL })).place(), Some(CHANNEL));
}

#[test]
fn a_destination_needs_one_credential_a_sound_team_and_sound_interactions() {
    assert_eq!(
        SlackKind::checked(options(&json!({})), &nothing),
        Err(Fault::here(
            "a Slack destination needs a token or a webhook; give one"
        ))
    );
    let teamed = options(&json!({ "token": { "secret": "T" }, "team": "nope" }));
    assert_eq!(
        SlackKind::checked(teamed, &nothing).map(|_| ()),
        Err(Fault::here(
            "\"nope\" is not a workspace id; give the workspace's id, a T and its letters and digits, such as \"T0123456789\""
        ))
    );
    let (_, faults) = SlackKind::checked(bot(&json!({})), &nothing).unwrap();
    assert_eq!(
        faults
            .iter()
            .map(|fault| fault.key.as_deref())
            .collect::<Vec<_>>(),
        [Some("interactions")]
    );
    let (checked, faults) = SlackKind::checked(bot(&posted()), &nothing).unwrap();
    assert_eq!(checked, bot(&posted()));
    assert_eq!(faults, []);
}

#[test]
fn a_route_through_a_bot_names_its_channel_and_a_subscription_mentions_subscribers() {
    let answering = bot(&posted());
    assert_eq!(
        messages(&SlackKind::placed(
            &answering,
            &route(&json!({})),
            placing(false)
        )),
        [(
            None,
            "a Slack bot posts to a channel; give channel = \"<the channel's id>\""
        )]
    );
    assert_eq!(
        SlackKind::placed(&webhook(&posted()), &route(&json!({})), placing(false)),
        []
    );
    let subscribed = route(&json!({ "channel": CHANNEL, "subscribe": {} }));
    assert_eq!(
        SlackKind::placed(&answering, &subscribed, placing(true)),
        []
    );
    assert_eq!(
        messages(&SlackKind::placed(&answering, &subscribed, placing(false))),
        [(
            Some("subscribe"),
            "a member subscribes to be mentioned; add mentions = { subscribers = true } to the route"
        )]
    );
    assert_eq!(
        messages(&SlackKind::placed(
            &bot(&json!({ "signing": { "secret": "S" }, "commands": ["latest"] })),
            &subscribed,
            placing(true)
        )),
        [(
            Some("subscribe"),
            "subscribing is answered through the destination's interactions; add \"subscribe\" to [destinations.slack.interactions] commands"
        )]
    );
}

fn interacted(options: &DestinationOptions, secrets: &FakeSecrets) -> Interacting {
    let interacted = pollster::block_on(options.interacts(&secret(), secrets))
        .unwrap()
        .unwrap();
    interacted.of::<Interacting>().unwrap().clone()
}

fn response(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

#[test]
fn commands_are_taken_with_the_secrets_they_are_signed_and_opened_with() {
    let secrets = FakeSecrets::default()
        .with("HERALD_SLACK_SIGNING", "signing")
        .with("HERALD_SLACK_APP", "xapp-1");
    let answering = interacted(&bot(&posted()), &secrets);
    assert_eq!(
        answering.signing_secret(),
        Some(&Secret::new("HERALD_SLACK_SIGNING", "signing"))
    );
    assert_eq!(answering.app_token(), None);
    assert_eq!(answering.sessions(), None);
    assert_eq!(
        answering.posted(Endpoint::Posted),
        Some(Ok((crate::SCHEME, "signing")))
    );
    assert_eq!(answering.posted(Endpoint::Pushed), None);
    assert!(answering.session().is_none());

    let socketed = interacted(&bot(&socket()), &secrets);
    assert_eq!(
        socketed.app_token(),
        Some(&Secret::new("HERALD_SLACK_APP", "xapp-1"))
    );
    assert_eq!(socketed.sessions(), Some("https://slack.com/api"));
    assert_eq!(
        socketed.posted(Endpoint::Posted),
        Some(Err("commands for such a destination"))
    );
    assert_eq!(
        pollster::block_on(
            options(&json!({ "token": { "secret": "T" } })).interacts(&secret(), &secrets)
        ),
        Ok(None)
    );
}

#[test]
fn commands_are_answered_through_the_response_url_they_came_with() {
    let interacting = Interacting::new(
        serde_json::from_value(posted()).unwrap(),
        Some("https://slack.test/"),
    );
    let reply = Reply {
        url: Secret::new("response", "https://hooks.slack.test/1"),
    };
    let parts = [Text {
        body: "one".to_owned(),
        html: None,
    }];
    assert_eq!(Interacts::api(&interacting), interacting.hooks());
    assert_eq!(Interacts::api(&interacting), "https://slack.test");
    assert_eq!(
        Interacts::answering(&interacting),
        Interacting::answering(&interacting)
    );
    assert_eq!(
        Interacts::answered(&interacting, &Replying::new(reply.clone()), &parts, false),
        [Interacting::answered(
            &interacting,
            &reply,
            &["one".to_owned()],
            false
        )]
    );
    assert_eq!(
        Interacts::answered(&interacting, &Replying::new(()), &parts, false),
        []
    );
    assert_eq!(
        Interacts::took(&interacting, &response(404, &json!("no_service"))),
        took(&response(404, &json!("no_service")))
    );
    assert!(Interacts::took(&interacting, &response(404, &json!("no_service"))).is_err());
}

#[test]
fn socket_mode_is_opened_with_the_app_level_token_and_heard_as_herald_answers_it() {
    let secrets = FakeSecrets::default().with("HERALD_SLACK_APP", "xapp-1");
    let socketed = interacted(&bot(&socket()), &secrets);
    let mut session = socketed.session().unwrap();
    assert_eq!(
        session.connect(),
        Connect::Opening(socketed.opening(&Secret::new("HERALD_SLACK_APP", "xapp-1")))
    );
    assert_eq!(
        session.opened(&response(
            200,
            &json!({ "ok": true, "url": "wss://wss.proxy.example/link" })
        )),
        Ok("wss://wss.proxy.example/link".to_owned())
    );
    assert_eq!(
        session.heard(
            &DestinationName::new("slack"),
            &json!({ "type": "hello" }).to_string()
        ),
        [Effect::Ready]
    );
    assert_eq!(
        session.unreachable(),
        "Slack's Socket Mode could not be opened"
    );
    let unopened = interacted(&bot(&json!({ "via": "socket" })), &secrets);
    assert!(unopened.session().is_none());
}
