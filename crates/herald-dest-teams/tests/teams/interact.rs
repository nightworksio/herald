//! What a Teams bot takes of each activity Teams posts to it, once the
//! token vouching for it is verified: only an activity of Teams naming the
//! service URL its token names, on the bot's own service origin, an id and
//! a conversation, of a type the bot takes; the commands members give it,
//! and its answers, each a reply carrying its token to that origin alone.

use herald_core::{Asked, Command, DestinationName, Place, Replies, Text, digest, every_command};
use herald_dest_teams::{
    ACTIVITY, CHANNEL, Cloud, DestinationOptions, Hosts, Interacting, MARKUP, Reply, bot_profile,
};
use herald_kinds::{Carried, DestinationConfig, Interacts, Post, Read, Replying, Said, Vouched};
use herald_ports::{Base, Method, Request, Response, Secret, SecretText};
use herald_testing::{FakeSecrets, ScriptedHttp};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};

const APP: &str = "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9";
const SERVICE: &str = "https://smba.trafficmanager.net/emea/";
const MEMBER: &str = "7FAF8AB2-3D56-4244-B585-20C8A42ED2B8";
const BOT: &str = "28:0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9";
const CHANNEL_ID: &str = "19:abc@thread.tacv2";

fn password() -> Secret {
    Secret::new(
        "HERALD_TEAMS_APP_PASSWORD",
        ["client", "-", "made"].concat(),
    )
}

fn bot() -> Interacting {
    Interacting::new(APP, password(), Cloud::Public.hosts()).unwrap()
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
    let high = Interacting::new(APP, password(), Cloud::GccHigh.hosts()).unwrap();
    assert_eq!(
        high.vouching().map(|vouching| vouching.issuer.as_str()),
        Some("https://api.botframework.us")
    );
    assert_eq!(bot().api(), "https://smba.trafficmanager.net/teams/");
    let unserved = Hosts {
        service: "http://smba.trafficmanager.net/teams/",
        ..Cloud::Public.hosts()
    };
    assert_eq!(Interacting::new(APP, password(), unserved), None);
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
            "the activity names a service URL that is not an https URL on the bot's service origin"
                .to_owned()
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
            "the activity names a service URL that is not an https URL on the bot's service origin"
                .to_owned()
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
fn a_bot_answers_in_its_card_markup_and_says_only_teams_error_code_back() {
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
    assert_eq!(bot.answering().commands, every_command());
    assert_eq!(bot.answering().replies, Replies::Public);
    let answer = |status, body: &str| Response {
        status,
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
    };
    assert_eq!(bot.accepted(&answer(201, "")), Ok(()));
    assert_eq!(
        bot.accepted(&answer(403, "")),
        Err("Microsoft Teams answered 403".to_owned())
    );
    assert_eq!(
        bot.accepted(&answer(
            403,
            r#"{"error":{"code":"BotNotInConversationRoster","message":"latest <at>x</at>"}}"#
        )),
        Err("Microsoft Teams answered 403: BotNotInConversationRoster".to_owned())
    );
    let long = "C".repeat(65);
    for uncoded in [
        r#"{"error":{"code":"Bad Argument"}}"#.to_owned(),
        r#"{"error":{"code":""}}"#.to_owned(),
        r#"{"error":{"code":7}}"#.to_owned(),
        r#"{"error":"Throttled"}"#.to_owned(),
        format!(r#"{{"error":{{"code":"{long}"}}}}"#),
        "Throttled".to_owned(),
    ] {
        assert_eq!(
            bot.accepted(&answer(429, &uncoded)),
            Err("Microsoft Teams answered 429".to_owned()),
            "{uncoded}"
        );
    }
    let coded = "C".repeat(64);
    assert_eq!(
        bot.accepted(&answer(
            400,
            &format!(r#"{{"error":{{"code":"{coded}"}}}}"#)
        )),
        Err(format!("Microsoft Teams answered 400: {coded}"))
    );
}

fn message(text: &str, conversation: &Value, entities: &Value) -> Value {
    json!({
        "type": "message",
        "id": "1481567603816",
        "serviceUrl": SERVICE,
        "channelId": CHANNEL,
        "conversation": conversation,
        "from": { "id": "29:member", "aadObjectId": MEMBER },
        "recipient": { "id": BOT, "name": "herald" },
        "text": text,
        "entities": entities,
    })
}

fn mentioning(account: &str) -> Value {
    json!([
        { "type": "clientInfo", "locale": "en-US" },
        { "type": "mention", "text": "<at>herald</at>", "mentioned": { "id": account, "name": "herald" } },
    ])
}

fn in_channel(text: &str) -> Value {
    message(
        text,
        &json!({ "id": format!("{CHANNEL_ID};messageid=1481567603816"), "conversationType": "channel" }),
        &mentioning(BOT),
    )
}

fn reply(conversation: &str) -> Reply {
    Reply {
        service: "https://smba.trafficmanager.net/emea".to_owned(),
        conversation: conversation.to_owned(),
        activity: "1481567603816".to_owned(),
    }
}

/// The command `read` asks, where it asks one, with where its answer goes.
fn asked(read: Read) -> Option<(Asked, Reply)> {
    let Read::Delivered { then, replayed, .. } = read else {
        panic!("{read:?} is not delivered");
    };
    assert_eq!(replayed, Said::ok("{}"));
    match *then {
        Read::Asked { mut asked, answer } => {
            assert_eq!((asked.len(), answer), (1, Said::ok("{}")));
            let (asked, replying) = asked.remove(0);
            Some((asked, replying.of::<Reply>().cloned().unwrap()))
        }
        Read::Said(said) => {
            assert_eq!(said, Said::ok("{}"));
            None
        }
        other => panic!("{other:?} is neither a command nor taken"),
    }
}

#[test]
fn a_command_after_the_bot_s_mention_acts_for_the_member_its_entra_object_id_names() {
    let conversation = format!("{CHANNEL_ID};messageid=1481567603816");
    let (given, answers) = asked(read(&in_channel("<at>herald</at> latest Herald share"))).unwrap();
    assert_eq!(
        given,
        Asked {
            destination: DestinationName::new("teams"),
            place: Place {
                id: conversation.clone(),
                parent: Some(CHANNEL_ID.to_owned()),
            },
            user: MEMBER.to_ascii_lowercase(),
            command: Command::Latest,
            project: Some("Herald".to_owned()),
            share: true,
            toggles: false,
            member: None,
        }
    );
    assert_eq!(answers, reply(&conversation));
    let (later, _) = asked(read(&in_channel("subscribe <at>herald</at>  herald"))).unwrap();
    assert_eq!(
        (later.command, later.project.as_deref(), later.share),
        (Command::Subscribe, Some("herald"), false)
    );
    let personal = message(
        "changelog",
        &json!({ "id": "a:1", "conversationType": "personal" }),
        &json!(null),
    );
    let (alone, answers) = asked(read(&personal)).unwrap();
    assert_eq!(
        (alone.command, alone.place, alone.project),
        (Command::Changelog, Place::new("a:1"), None)
    );
    assert_eq!(answers, reply("a:1"));
    let group = message(
        "<at>herald</at> latest",
        &json!({ "id": "19:chat@thread.v2", "conversationType": "groupChat" }),
        &mentioning(BOT),
    );
    assert_eq!(
        asked(read(&group)).unwrap().0.place,
        Place::new("19:chat@thread.v2")
    );
    let unthreaded = message(
        "<at>herald</at> latest",
        &json!({ "id": ";messageid=1" }),
        &mentioning(BOT),
    );
    assert_eq!(
        asked(read(&unthreaded)).unwrap().0.place,
        Place::new(";messageid=1")
    );
}

#[test]
fn a_message_not_addressed_to_the_bot_or_from_no_member_it_can_name_asks_nothing() {
    let unaddressed = message(
        "latest herald",
        &json!({ "id": CHANNEL_ID, "conversationType": "channel" }),
        &json!([]),
    );
    let elsewhere = message(
        "<at>herald</at> latest herald",
        &json!({ "id": CHANNEL_ID, "conversationType": "channel" }),
        &mentioning("29:someone-else"),
    );
    let mut no_member = in_channel("<at>herald</at> latest");
    no_member["from"] = json!({ "id": "29:member" });
    let mut unshaped = in_channel("<at>herald</at> latest");
    unshaped["from"]["aadObjectId"] = json!("7faf8ab2");
    let mut itself = in_channel("<at>herald</at> latest");
    itself["from"]["id"] = json!(BOT);
    let mut unrecipient = in_channel("<at>herald</at> latest");
    unrecipient.as_object_mut().unwrap().remove("recipient");
    let mut sender = in_channel("<at>herald</at> latest");
    sender.as_object_mut().unwrap().remove("from");
    let mut textless = in_channel("");
    textless.as_object_mut().unwrap().remove("text");
    let mut unmentioned = in_channel("latest");
    unmentioned["entities"] =
        json!([{ "type": "mention", "text": "<at>herald</at>", "mentioned": { "id": BOT } }]);
    let mut empty = in_channel("<at>herald</at> latest");
    empty["entities"] = json!([{ "type": "mention", "text": "", "mentioned": { "id": BOT } }]);
    let mut untyped = in_channel("<at>herald</at> latest");
    untyped["entities"] =
        json!([{ "type": "hashtag", "text": "<at>herald</at>", "mentioned": { "id": BOT } }]);
    let mut nameless = in_channel("<at>herald</at> latest");
    nameless["entities"] =
        json!([{ "type": "mention", "text": "<at>herald</at>", "mentioned": {} }]);
    let long = format!("<at>herald</at> latest {}", "h".repeat(1_024));
    for message in [
        unaddressed,
        elsewhere,
        no_member,
        unshaped,
        itself,
        unrecipient,
        sender,
        textless,
        unmentioned,
        empty,
        untyped,
        nameless,
        in_channel("<at>herald</at> hello"),
        in_channel("<at>herald</at> latest herald share now"),
        in_channel(&long),
    ] {
        assert_eq!(asked(read(&message)), None, "{message}");
    }
    let fitting = format!("<at>herald</at> latest {}", "h".repeat(1_024 - 23));
    assert_eq!(fitting.len(), 1_024);
    assert!(asked(read(&in_channel(&fitting))).is_some());
    let mut queried = in_channel("<at>herald</at> latest");
    queried["serviceUrl"] = json!("https://smba.trafficmanager.net/emea/?x=1");
    assert_eq!(
        read_by(
            &bot(),
            &queried,
            Some(&vouched("https://smba.trafficmanager.net/emea/?x=1"))
        ),
        Read::Unvouched(
            "the activity names a service URL that is not an https URL on the bot's service origin"
                .to_owned()
        )
    );
}

fn part(body: &str) -> Text {
    Text {
        body: body.to_owned(),
        html: None,
    }
}

#[test]
fn an_answer_replies_to_the_member_s_message_as_cards_through_the_service_its_activity_named() {
    let bot = bot();
    let conversation = format!("{CHANNEL_ID};messageid=1481567603816");
    let requests = bot.answered(
        &Replying::new(reply(&conversation)),
        &[
            part("**herald 1.0**"),
            part("\\<at\\>x"),
            part(&"a".repeat(40_001)),
        ],
        false,
    );
    let url = "https://smba.trafficmanager.net/emea/v3/conversations/19%3Aabc%40thread.tacv2%3Bmessageid%3D1481567603816/activities/1481567603816";
    assert_eq!(requests.len(), 2);
    for (request, said) in requests.iter().zip(["**herald 1.0**", "\\<at\\>x"]) {
        assert_eq!(
            (request.method, request.url.redacted()),
            (Method::Post, url.to_owned())
        );
        assert_eq!(
            request
                .headers
                .iter()
                .find(|(name, _)| name == "Authorization"),
            None
        );
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["type"], "message");
        let card = &body["attachments"][0];
        assert_eq!(
            card["contentType"],
            "application/vnd.microsoft.card.adaptive"
        );
        assert_eq!(
            card["content"]["body"],
            json!([{ "type": "TextBlock", "text": said, "wrap": true }])
        );
    }
    assert_eq!(bot.answered(&Replying::new(1_u8), &[part("a")], true), []);
    for (conversation, activity) in [("", "1"), ("19:a", ".."), (".", "1")] {
        let unsegmented = Reply {
            conversation: conversation.to_owned(),
            activity: activity.to_owned(),
            ..reply("x")
        };
        assert_eq!(
            bot.answered(&Replying::new(unsegmented), &[part("a")], true),
            [],
            "{conversation} {activity}"
        );
    }
}

fn signed_in(token: &str) -> Response {
    Response {
        status: 200,
        headers: Vec::new(),
        body: json!({ "access_token": token, "expires_in": 3600 })
            .to_string()
            .into_bytes(),
    }
}

fn answering_request(url: &str) -> Request {
    Request::new(Method::Post, SecretText::plain(url))
}

#[test]
fn an_answer_carries_the_bot_s_token_to_its_service_origin_alone_signing_in_once() {
    let signed = bot();
    assert_eq!(
        signed.reaches(),
        Base::Among(&[
            "https://login.microsoftonline.com".to_owned(),
            "https://smba.trafficmanager.net".to_owned(),
        ])
    );
    let token = ["made", "-", "at", "-", "run"].concat();
    let http = ScriptedHttp::new(vec![signed_in(&token)]);
    let now = Timestamp::from_second(1_800_000_000).unwrap();
    let url = "https://smba.trafficmanager.net/emea/v3/conversations/a/activities/1";
    for _ in 0..2 {
        let ready =
            pollster::block_on(signed.authorised(&http, answering_request(url), now)).unwrap();
        assert_eq!(ready.url.redacted(), url);
        assert_eq!(
            ready.headers,
            [(
                "Authorization".to_owned(),
                SecretText::plain("Bearer ").then_secret(Secret::new(
                    "the Microsoft Teams bot's token",
                    token.clone()
                ))
            )]
        );
    }
    let asked = http.asked();
    assert_eq!(
        asked,
        [(
            "https://login.microsoftonline.com/botframework.com/oauth2/v2.0/token".to_owned(),
            None
        )]
    );
    for elsewhere in [
        "https://login.microsoftonline.com/v3/conversations/a/activities/1",
        "https://attacker.example/v3/conversations/a/activities/1",
        "http://smba.trafficmanager.net/emea/v3/conversations/a/activities/1",
        "https://smba.trafficmanager.net.attacker.example/v3",
    ] {
        assert_eq!(
            pollster::block_on(signed.authorised(&http, answering_request(elsewhere), now)),
            Err("an answer to a command goes off the bot's service origin, so it carries no token and is not sent".to_owned()),
            "{elsewhere}"
        );
    }
    assert_eq!(http.asked().len(), 1);
    let refused = ScriptedHttp::new(vec![Response {
        status: 401,
        headers: Vec::new(),
        body: br#"{"error":"invalid_client","error_codes":[7000215],"error_description":"latest"}"#
            .to_vec(),
    }]);
    let unsigned =
        pollster::block_on(bot().authorised(&refused, answering_request(url), now)).unwrap_err();
    assert!(
        unsigned.contains("invalid_client (AADSTS7000215)"),
        "{unsigned}"
    );
    assert!(!unsigned.contains("latest"), "{unsigned}");
    let silent = ScriptedHttp::new(Vec::new());
    assert!(pollster::block_on(bot().authorised(&silent, answering_request(url), now)).is_err());
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
    assert_eq!(interacted.answering().commands, every_command());
    let narrowed = table(
        json!({ "interactions": { "commands": ["latest"], "prereleases": true, "template": "short" } }),
    );
    let answering = pollster::block_on(narrowed.interacts(&credential, &secrets))
        .unwrap()
        .unwrap()
        .answering();
    assert_eq!(
        (
            answering.commands,
            answering.prereleases,
            answering
                .template
                .map(|template| template.as_str().to_owned())
        ),
        (vec![Command::Latest], true, Some("short".to_owned()))
    );
    assert_eq!(
        narrowed.answering().map(|answering| answering.replies),
        Some(Replies::Public)
    );
    assert_eq!(table(json!({})).answering(), None);
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
