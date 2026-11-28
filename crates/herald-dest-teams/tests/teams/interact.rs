//! What a Teams bot takes of each activity Teams posts to it, once the
//! token vouching for it is verified: only an activity of Teams naming the
//! service URL its token names, on the bot's own service origin, an id and
//! a conversation, of a type the bot takes.

use herald_core::{DestinationName, Text, digest};
use herald_dest_teams::{
    ACTIVITY, CHANNEL, Cloud, DestinationOptions, Hosts, Interacting, MARKUP, bot_profile,
};
use herald_kinds::{Carried, DestinationConfig, Interacts, Post, Read, Replying, Said, Vouched};
use herald_ports::{Response, Secret};
use herald_testing::FakeSecrets;
use jiff::SignedDuration;
use serde_json::{Value, json};

const APP: &str = "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9";
const SERVICE: &str = "https://smba.trafficmanager.net/emea/";

fn bot() -> Interacting {
    Interacting::new(APP, Cloud::Public.hosts()).unwrap()
}

fn activity(kind: &str) -> Value {
    json!({
        "type": kind,
        "id": "1481567603816",
        "serviceUrl": SERVICE,
        "channelId": CHANNEL,
        "conversation": { "id": "19:abc@thread.tacv2" },
        "from": { "id": "29:member" },
    })
}

fn vouched(service: &str) -> Vouched {
    Vouched {
        claims: vec![("serviceUrl", service.to_owned())],
    }
}

fn read_by(bot: &Interacting, body: &Value, vouched: Option<&Vouched>) -> Read {
    bot.read(
        &DestinationName::new("teams"),
        Post {
            method: "POST",
            path: &[],
            body: body.to_string().as_bytes(),
            vouched,
        },
    )
}

fn read(body: &Value) -> Read {
    read_by(&bot(), body, Some(&vouched(SERVICE)))
}

fn taken() -> Read {
    Read::Delivered {
        delivery: digest("19:abc@thread.tacv2\n1481567603816"),
        replayed: Said::ok("{}"),
        then: Box::new(Read::Said(Said::ok("{}"))),
    }
}

#[test]
fn a_bot_is_vouched_for_by_its_cloud_s_bot_framework_and_meant_for_its_app_alone() {
    let vouching = bot().vouching().cloned().unwrap();
    assert_eq!(
        (
            vouching.name,
            vouching.metadata.as_str(),
            vouching.issuer.as_str(),
            vouching.audience.as_str()
        ),
        (
            "teams",
            "https://login.botframework.com/v1/.well-known/openidconfiguration",
            "https://api.botframework.com",
            APP
        )
    );
    assert_eq!(
        (vouching.endorsement, vouching.claims, vouching.body),
        ("msteams", &["serviceUrl"][..], ACTIVITY)
    );
    assert_eq!(vouching.skew, SignedDuration::from_mins(5));
    assert_eq!(vouching.keys_for, SignedDuration::from_hours(24));
    assert_eq!(vouching.keys_retry, SignedDuration::from_mins(5));
    assert_eq!(vouching.keys_most, 4 * 1024 * 1024);
    assert_eq!(ACTIVITY, 256 * 1024);
    let high = Interacting::new(APP, Cloud::GccHigh.hosts()).unwrap();
    assert_eq!(
        high.vouching().map(|vouching| vouching.issuer.as_str()),
        Some("https://api.botframework.us")
    );
    assert_eq!(bot().api(), "https://smba.trafficmanager.net/teams/");
    let unserved = Hosts {
        service: "http://smba.trafficmanager.net/teams/",
        ..Cloud::Public.hosts()
    };
    assert_eq!(Interacting::new(APP, unserved), None);
}

#[test]
fn an_activity_of_teams_on_the_service_its_token_names_is_taken_once_by_its_id() {
    for kind in [
        "message",
        "conversationUpdate",
        "installationUpdate",
        "messageReaction",
        "messageUpdate",
        "messageDelete",
        "typing",
        "endOfConversation",
    ] {
        assert_eq!(read(&activity(kind)), taken(), "{kind}");
    }
    let other = json!({
        "type": "message",
        "id": "2",
        "serviceUrl": SERVICE,
        "channelId": CHANNEL,
        "conversation": { "id": "19:abc@thread.tacv2" },
    });
    assert_eq!(
        read(&other),
        Read::Delivered {
            delivery: digest("19:abc@thread.tacv2\n2"),
            replayed: Said::ok("{}"),
            then: Box::new(Read::Said(Said::ok("{}"))),
        }
    );
}

#[test]
fn an_activity_naming_a_service_its_token_does_not_vouch_for_is_refused_as_unauthorised() {
    let elsewhere =
        Read::Unvouched("the activity names a service URL its token does not vouch for".to_owned());
    let mut moved = activity("message");
    moved["serviceUrl"] = json!("https://attacker.example/teams/");
    assert_eq!(read(&moved), elsewhere);
    let mut unnamed = activity("message");
    unnamed.as_object_mut().unwrap().remove("serviceUrl");
    assert_eq!(read(&unnamed), elsewhere);
    assert_eq!(
        read_by(&bot(), &unnamed, Some(&Vouched::default())),
        elsewhere
    );
    let off = "https://attacker.example/teams/";
    assert_eq!(
        read_by(&bot(), &moved, Some(&vouched(off))),
        Read::Unvouched(
            "the activity names a service URL that is not on the bot's service origin".to_owned()
        )
    );
    let mut plain = activity("message");
    plain["serviceUrl"] = json!("http://smba.trafficmanager.net/emea/");
    assert_eq!(
        read_by(
            &bot(),
            &plain,
            Some(&vouched("http://smba.trafficmanager.net/emea/"))
        ),
        Read::Unvouched(
            "the activity names a service URL that is not on the bot's service origin".to_owned()
        )
    );
    assert_eq!(
        read_by(&bot(), &activity("message"), None),
        Read::Unvouched("no token vouched for the activity".to_owned())
    );
}

#[test]
fn an_activity_of_another_channel_type_or_shape_is_refused() {
    let refused = |why: &str| Read::Refused(why.to_owned());
    let mut webchat = activity("message");
    webchat["channelId"] = json!("webchat");
    assert_eq!(
        read(&webchat),
        refused("the activity is not from Microsoft Teams")
    );
    let mut unchannelled = activity("message");
    unchannelled.as_object_mut().unwrap().remove("channelId");
    assert_eq!(
        read(&unchannelled),
        refused("the activity is not from Microsoft Teams")
    );
    for kind in ["invoke", "event", "trace", "Message", ""] {
        assert_eq!(
            read(&activity(kind)),
            refused("the bot takes no activity of this type"),
            "{kind}"
        );
    }
    let mut untyped = activity("message");
    untyped.as_object_mut().unwrap().remove("type");
    assert_eq!(
        read(&untyped),
        refused("the bot takes no activity of this type")
    );
    let unnamed = refused("the activity names no id or conversation");
    for (key, value) in [
        ("id", json!("")),
        ("id", Value::Null),
        ("conversation", json!({ "id": "" })),
        ("conversation", json!({})),
        ("conversation", Value::Null),
    ] {
        let mut shaped = activity("message");
        shaped[key] = value;
        assert_eq!(read(&shaped), unnamed, "{key}");
    }
    let unread = refused("the request is not a Bot Framework activity");
    for body in [json!([]), json!("message"), json!({ "type": 7 })] {
        assert_eq!(read(&body), unread, "{body}");
    }
    let bot = bot();
    let post = |method: &'static str, path: &'static [&'static str]| {
        bot.read(
            &DestinationName::new("teams"),
            Post {
                method,
                path,
                body: b"{}",
                vouched: None,
            },
        )
    };
    assert_eq!(post("GET", &[]), Read::Unmethodical);
    assert_eq!(post("POST", &["x"]), Read::Missing("such path"));
}

#[test]
fn a_bot_answers_nothing_yet_in_its_card_markup_and_reads_teams_answers() {
    let bot = bot();
    let (markup, room) = bot.room();
    let profile = bot_profile(false);
    assert_eq!(markup, MARKUP);
    assert_eq!(
        (room.measure, room.first, room.rest),
        (
            profile.measure,
            profile.limits.text,
            profile.limits.followup
        )
    );
    assert_eq!(bot.answering().commands, []);
    assert_eq!(
        bot.answered(
            &Replying::new(1_u8),
            &[Text {
                body: "a".to_owned(),
                html: None,
            }],
            true,
        ),
        []
    );
    let answer = |status| Response {
        status,
        headers: Vec::new(),
        body: Vec::new(),
    };
    assert_eq!(bot.accepted(&answer(201)), Ok(()));
    assert_eq!(
        bot.accepted(&answer(403)),
        Err("Microsoft Teams answered 403".to_owned())
    );
}

#[test]
fn a_bot_takes_activities_where_its_table_says_so_and_a_layer_serves_an_endpoint() {
    let table = |extra: Value| {
        let mut table = json!({
            "app_id": APP,
            "app_password": { "secret": "HERALD_TEAMS_APP_PASSWORD" },
            "service": "https://smba.example/teams/",
            "openid": "https://login.example/v1/.well-known/openidconfiguration",
            "issuer": "https://api.example",
        });
        table
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().cloned().unwrap_or_default());
        serde_json::from_value::<DestinationOptions>(table).unwrap()
    };
    let taking = table(json!({ "interactions": { "via": "http" } }));
    assert_eq!(
        taking.carried(),
        Some(Carried {
            posted: true,
            what: "activities Teams posts to the bot",
            session: None,
        })
    );
    let credential = Secret::new("HERALD_TEAMS_APP_PASSWORD", "x");
    let secrets = FakeSecrets::default();
    let interacted = pollster::block_on(taking.interacts(&credential, &secrets))
        .unwrap()
        .unwrap();
    let vouching = interacted.vouching().unwrap();
    assert_eq!(
        (vouching.metadata.as_str(), vouching.issuer.as_str()),
        (
            "https://login.example/v1/.well-known/openidconfiguration",
            "https://api.example"
        )
    );
    assert_eq!(interacted.api(), "https://smba.example/teams/");
    let silent = table(json!({}));
    assert_eq!(silent.carried(), None);
    assert!(
        pollster::block_on(silent.interacts(&credential, &secrets))
            .unwrap()
            .is_none()
    );
    let unread = table(json!({ "interactions": {}, "openid": "http://login.example/" }));
    assert!(
        pollster::block_on(unread.interacts(&credential, &secrets))
            .unwrap()
            .is_none()
    );
}
