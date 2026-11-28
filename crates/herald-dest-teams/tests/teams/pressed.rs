//! A press of herald's subscribe button: the one invoke a Teams bot takes,
//! `adaptiveCard/action` of herald's verb naming a project, acted on only
//! for the member the verified activity names and answered at once.

use herald_core::{Asked, Command, DestinationName, Phrase, Place, digest};
use herald_dest_teams::{CHANNEL, Cloud, Interacting, Interactions, Reply, SUBSCRIBE};
use herald_kinds::{Interacts, Post, Read, Said, Vouched};
use herald_ports::Secret;
use serde_json::{Value, json};

const APP: &str = "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9";
const SERVICE: &str = "https://smba.trafficmanager.net/emea/";
const MEMBER: &str = "7FAF8AB2-3D56-4244-B585-20C8A42ED2B8";
const BOT: &str = "28:0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9";
const CONVERSATION: &str = "19:abc@thread.tacv2;messageid=1481567603816";
const CARD: &str = "1481567603816";

fn bot() -> Interacting {
    Interacting::new(
        APP,
        Secret::new(
            "HERALD_TEAMS_APP_PASSWORD",
            ["client", "-", "made"].concat(),
        ),
        Cloud::Public.hosts(),
    )
    .unwrap()
}

fn press(project: &Value) -> Value {
    json!({
        "type": "invoke",
        "name": "adaptiveCard/action",
        "id": "f:1",
        "serviceUrl": SERVICE,
        "channelId": CHANNEL,
        "conversation": { "id": CONVERSATION, "conversationType": "channel" },
        "from": { "id": "29:member", "aadObjectId": MEMBER },
        "recipient": { "id": BOT },
        "replyToId": CARD,
        "value": {
            "action": { "type": "Action.Execute", "verb": SUBSCRIBE, "data": { "project": project } },
            "trigger": "manual",
        },
    })
}

fn read_by(bot: &Interacting, body: &Value) -> Read {
    bot.read(
        &DestinationName::new("teams"),
        Post {
            method: "POST",
            path: &[],
            body: body.to_string().as_bytes(),
            vouched: Some(&Vouched {
                claims: vec![("serviceUrl", SERVICE.to_owned())],
            }),
        },
    )
}

fn shown(said: &str) -> Said {
    Said::ok(
        json!({
            "statusCode": 200,
            "type": "application/vnd.microsoft.activity.message",
            "value": said,
        })
        .to_string(),
    )
}

fn untaken() -> Said {
    Said::ok(
        json!({
            "statusCode": 400,
            "type": "application/vnd.microsoft.error",
            "value": {
                "code": "BadRequest",
                "message": "herald cannot act on this press: Microsoft Teams named no member or message herald can take",
            },
        })
        .to_string(),
    )
}

/// What `read` asks, and what it answers at once and on a replay.
fn asked(read: Read) -> (Option<(Asked, Reply)>, Said) {
    let Read::Delivered {
        delivery,
        replayed,
        then,
    } = read
    else {
        panic!("{read:?} is not delivered");
    };
    assert_eq!(delivery, digest(&format!("{CONVERSATION}\nf:1")));
    match *then {
        Read::Asked { mut asked, answer } => {
            assert_eq!((asked.len(), &answer), (1, &replayed));
            let (asked, replying) = asked.remove(0);
            (
                Some((asked, replying.of::<Reply>().cloned().unwrap())),
                answer,
            )
        }
        Read::Said(answer) => {
            assert_eq!(answer, replayed);
            (None, answer)
        }
        other => panic!("{other:?} is neither a command nor taken"),
    }
}

#[test]
fn a_press_subscribes_the_member_its_activity_names_and_replies_under_the_card() {
    let (given, answer) = asked(read_by(&bot(), &press(&json!("herald"))));
    let (given, reply) = given.unwrap();
    assert_eq!(
        given,
        Asked {
            destination: DestinationName::new("teams"),
            place: Place {
                id: CONVERSATION.to_owned(),
                parent: Some("19:abc@thread.tacv2".to_owned()),
            },
            user: MEMBER.to_ascii_lowercase(),
            command: Command::Subscribe,
            project: Some("herald".to_owned()),
            share: false,
            toggles: true,
            member: None,
        }
    );
    assert_eq!(
        reply,
        Reply {
            service: "https://smba.trafficmanager.net/emea".to_owned(),
            conversation: CONVERSATION.to_owned(),
            activity: CARD.to_owned(),
        }
    );
    assert_eq!(
        answer,
        shown("herald is on it; its answer follows in this conversation.")
    );
    let worded = bot().taking(
        serde_json::from_value::<Interactions>(json!({ "wording": { "pressed": "Bezig." } }))
            .unwrap()
            .answering(),
    );
    assert_eq!(
        asked(read_by(&worded, &press(&json!("herald")))).1,
        shown("Bezig.")
    );
    let mut untriggered = press(&json!("herald"));
    untriggered["value"]
        .as_object_mut()
        .unwrap()
        .remove("trigger");
    assert!(asked(read_by(&bot(), &untriggered)).0.is_some());
    let longest = "h".repeat(256);
    assert_eq!(
        asked(read_by(&bot(), &press(&json!(longest))))
            .0
            .unwrap()
            .0
            .project,
        Some(longest)
    );
    assert_eq!(Phrase::Pressed.name(), "pressed");
}

#[test]
fn an_invoke_that_is_not_a_press_of_herald_s_button_is_refused() {
    let refused = Read::Refused("the bot takes no activity of this type".to_owned());
    let mut cases: Vec<Value> = [
        json!(""),
        json!(" herald"),
        json!("herald\n"),
        json!("h".repeat(257)),
        json!(7),
        Value::Null,
    ]
    .iter()
    .map(press)
    .collect();
    for (path, value) in [
        ("/name", json!("task/fetch")),
        ("/name", Value::Null),
        ("/value/action/type", json!("Action.Submit")),
        ("/value/action/verb", json!("approve")),
        ("/value/trigger", json!("automatic")),
        ("/value/trigger", json!(1)),
        ("/value/action/data", json!("herald")),
        ("/value/action", json!({})),
        ("/value", json!("herald")),
        ("/value", Value::Null),
    ] {
        let mut shaped = press(&json!("herald"));
        *shaped.pointer_mut(path).unwrap() = value;
        cases.push(shaped);
    }
    for case in cases {
        assert_eq!(read_by(&bot(), &case), refused, "{case}");
    }
}

#[test]
fn a_press_naming_no_member_or_card_herald_can_take_is_answered_as_untaken() {
    let mut cases = Vec::new();
    for (path, value) in [
        ("/from", json!({ "id": "29:member" })),
        ("/from", json!({ "id": BOT, "aadObjectId": MEMBER })),
        ("/from/aadObjectId", json!("7faf8ab2")),
        ("/replyToId", json!("")),
        ("/replyToId", Value::Null),
        ("/recipient", Value::Null),
    ] {
        let mut shaped = press(&json!("herald"));
        *shaped.pointer_mut(path).unwrap() = value;
        cases.push(shaped);
    }
    let mut unreplied = press(&json!("herald"));
    unreplied.as_object_mut().unwrap().remove("replyToId");
    cases.push(unreplied);
    for case in cases {
        assert_eq!(asked(read_by(&bot(), &case)), (None, untaken()), "{case}");
    }
}
