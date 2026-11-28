//! A Teams bot's cards mention the members subscribed by their Microsoft
//! Entra object id, under the label the wording gives, and carry the button
//! that subscribes where the route offers it; a webhook does neither.

use std::collections::BTreeMap;
use std::sync::Mutex;

use herald_core::{Announcement, Entry, Facts, MentionStyle, Phrase, Step, Subscribe, Wording};
use herald_dest_teams::{
    Bot, Cloud, DestinationOptions, RouteOptions, SUBSCRIBE, Teams, TeamsKind, bot_profile, profile,
};
use herald_kinds::{DestinationConfig, DestinationKind, Fault, Placing};
use herald_ports::{Http, HttpError, Request, Response, Secret};
use herald_testing::sample;
use jiff::Timestamp;
use serde_json::{Value, json};

const MEMBER: &str = "7faf8ab2-3d56-4244-b585-20c8a42ed2b8";
const OTHER: &str = "0e3c5d6f-0000-4000-8000-000000000002";

/// An `Http` that keeps each body sent and answers each request with a
/// token and a message id.
#[derive(Default)]
struct Bodies(Mutex<Vec<Vec<u8>>>);

impl Http for Bodies {
    fn send(
        &self,
        request: Request,
    ) -> impl std::future::Future<Output = Result<Response, HttpError>> + Send {
        self.0.lock().unwrap().push(request.body);
        std::future::ready(Ok(Response {
            status: 200,
            headers: Vec::new(),
            body: br#"{"id":"9","access_token":"t","expires_in":3600}"#.to_vec(),
        }))
    }
}

fn route(subscribe: Option<Subscribe>) -> RouteOptions {
    RouteOptions {
        conversation: Some("19:abc@thread.tacv2".to_owned()),
        subscribe,
        ..RouteOptions::default()
    }
}

fn bot(subscribe: Option<Subscribe>, wording: Wording) -> Teams {
    Teams::bot(
        Bot::new(
            "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9",
            Secret::new("HERALD_TEAMS_APP_PASSWORD", ["app-", "password"].concat()),
            Cloud::Public.hosts(),
            &route(subscribe),
        )
        .worded(wording),
    )
}

fn mentioning(after: Vec<Vec<String>>) -> Announcement {
    Announcement {
        mentioned: vec![
            MEMBER.to_owned(),
            "29:not-an-object-id".to_owned(),
            OTHER.to_owned(),
        ],
        mentioned_after: after,
        ..sample::announcement()
    }
}

/// The card content `teams` sends for `step` of `announcement`.
fn carded(teams: &Teams, step: &Step, announcement: &Announcement) -> Value {
    let bodies = Bodies::default();
    let entry = Entry::new(announcement.key).landed(
        Step::Post,
        Facts {
            message: Some("1700000000001".to_owned()),
            ..Facts::default()
        },
    );
    pollster::block_on(teams.send(&bodies, (step, announcement, &entry), Timestamp::UNIX_EPOCH))
        .unwrap();
    let sent = bodies.0.lock().unwrap().last().cloned().unwrap();
    serde_json::from_slice::<Value>(&sent).unwrap()["attachments"][0]["content"].clone()
}

fn entity(label: &str, id: &str) -> Value {
    json!({
        "type": "mention",
        "text": format!("<at>{label}</at>"),
        "mentioned": { "id": id, "name": label },
    })
}

#[test]
fn a_bot_s_card_mentions_each_object_id_numbered_under_the_worded_label_and_offers_its_button() {
    let worded = Wording::of(Some(&BTreeMap::from([
        (Phrase::Mentioned, "abonnee".to_owned()),
        (Phrase::SubscribeButton, "Abonneren".to_owned()),
    ])));
    let teams = bot(Some(Subscribe::default()), worded);
    let announcement = mentioning(Vec::new());
    let edit = Step::Edit {
        revision: "r".to_owned(),
        part: 0,
    };
    for step in [Step::Post, edit] {
        let card = carded(&teams, &step, &announcement);
        assert_eq!(
            card["msteams"]["entities"],
            json!([entity("abonnee 1", MEMBER), entity("abonnee 2", OTHER)]),
            "{step:?}"
        );
        assert_eq!(
            card["body"].as_array().unwrap().last().unwrap()["text"],
            "<at>abonnee 1</at> <at>abonnee 2</at>"
        );
        let actions = card["actions"].as_array().unwrap();
        assert_eq!(
            actions.last().unwrap(),
            &json!({
                "type": "Action.Execute",
                "title": "Abonneren",
                "verb": SUBSCRIBE,
                "data": { "project": "php-mutation-gate" },
            })
        );
        assert!(actions.len() > 1);
        assert_eq!(card["version"], "1.5");
    }
    assert_eq!(SUBSCRIBE, "herald_subscribe");
}

#[test]
fn a_card_with_no_object_id_to_mention_or_no_button_writes_neither() {
    let unbuttoned = bot(Some(Subscribe { button: false }), Wording::default());
    let alone = Announcement {
        mentioned: vec!["29:not-an-object-id".to_owned()],
        buttons: Vec::new(),
        ..sample::announcement()
    };
    let card = carded(&unbuttoned, &Step::Post, &alone);
    assert_eq!((card.get("msteams"), card.get("actions")), (None, None));
    assert_eq!(card["version"], "1.0");
    let shown = card["body"].as_array().unwrap().last().unwrap()["text"].clone();
    assert!(
        shown.as_str().unwrap().contains("releases/tag/v0.1.0"),
        "{shown}"
    );
    let buttoned = bot(Some(Subscribe::default()), Wording::default());
    let card = carded(&buttoned, &Step::Post, &alone);
    let actions = card["actions"].as_array().unwrap();
    assert_eq!(
        (actions.len(), actions[0]["title"].clone()),
        (1, json!("Subscribe"))
    );
    let shown = card["body"].as_array().unwrap().last().unwrap()["text"].clone();
    assert!(
        shown.as_str().unwrap().contains("releases/tag/v0.1.0"),
        "{shown}"
    );
}

#[test]
fn members_mentioned_after_the_parts_are_mentioned_in_a_card_of_their_own() {
    let teams = bot(None, Wording::default());
    let announcement = mentioning(vec![
        vec![OTHER.to_owned()],
        vec!["29:not-an-object-id".to_owned()],
    ]);
    let card = carded(&teams, &Step::Chunk(2), &announcement);
    assert_eq!(
        (
            card["body"][0]["text"].clone(),
            card["msteams"]["entities"].clone()
        ),
        (
            json!("<at>subscriber 1</at>"),
            json!([entity("subscriber 1", OTHER)])
        )
    );
    assert_eq!(card.get("actions"), None);
    let empty = carded(&teams, &Step::Chunk(3), &announcement);
    assert_eq!(
        (empty["body"][0]["text"].clone(), empty.get("msteams")),
        (json!(""), None)
    );
    let part = carded(&teams, &Step::Chunk(1), &announcement);
    assert_eq!(
        (part["body"][0]["text"].clone(), part.get("msteams")),
        (json!("- the gate"), None)
    );
}

#[test]
fn a_webhook_mentions_no_one_and_a_bot_mentions_users_and_takes_commands() {
    let webhook = Teams::new(Secret::new(
        "HERALD_TEAMS_WEBHOOK",
        "https://flows.example/hook",
    ));
    let card = carded(&webhook, &Step::Post, &mentioning(Vec::new()));
    assert_eq!(card.get("msteams"), None);
    assert!(
        card["actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|action| action["type"] == "Action.OpenUrl")
    );
    assert_eq!(profile().features.mentions, MentionStyle::None);
    assert!(!profile().features.interactions);
    let features = bot_profile(false).features;
    assert!(features.interactions);
    let MentionStyle::Users(room) = features.mentions else {
        panic!("a bot mentions users");
    };
    assert_eq!((room.first, room.rest, room.copies), (3_000, 30_000, 1));
    assert_eq!(room.overhead, 3 * (Phrase::MENTIONED * 4 + 14) + 64);
    let spread = room.spread(vec![MEMBER.to_owned(); 70]);
    assert_eq!(room.cost(MEMBER), 36 + 490);
    assert_eq!(spread.iter().map(Vec::len).collect::<Vec<_>>(), [5, 57, 8]);
}

fn placed(options: &DestinationOptions, route: &RouteOptions, subscribers: bool) -> Vec<Fault> {
    TeamsKind::placed(
        options,
        route,
        Placing {
            destination: "teams",
            subscribers,
        },
    )
}

#[test]
fn a_bot_s_route_offers_subscribing_where_it_mentions_and_answers_and_a_webhook_s_never() {
    let interacting: DestinationOptions = serde_json::from_value(json!({
        "app_id": "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9",
        "app_password": { "secret": "HERALD_TEAMS_APP_PASSWORD" },
        "interactions": {},
    }))
    .unwrap();
    let offered = route(Some(Subscribe::default()));
    assert_eq!(placed(&interacting, &offered, true), []);
    assert_eq!(
        placed(&interacting, &offered, false),
        [Fault::at(
            "subscribe",
            "a member subscribes to be mentioned; add mentions = { subscribers = true } to the route"
        )]
    );
    let silent: DestinationOptions = serde_json::from_value(json!({
        "app_id": "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9",
        "app_password": { "secret": "HERALD_TEAMS_APP_PASSWORD" },
    }))
    .unwrap();
    assert_eq!(
        placed(&silent, &offered, true),
        [Fault::at(
            "subscribe",
            "subscribing is answered through the destination's interactions; add \"subscribe\" to [destinations.teams.interactions] commands"
        )]
    );
    assert_eq!(placed(&silent, &route(None), true), []);
    let webhook: DestinationOptions =
        serde_json::from_value(json!({ "webhook": { "secret": "W" } })).unwrap();
    let unconversed = RouteOptions {
        subscribe: Some(Subscribe::default()),
        ..RouteOptions::default()
    };
    assert_eq!(
        placed(&webhook, &unconversed, true),
        [Fault::at(
            "subscribe",
            "a Workflows webhook only posts, so no member subscribes through it; drop it"
        )]
    );
    let written = serde_json::to_string(&offered).unwrap();
    assert_eq!(
        written,
        r#"{"conversation":"19:abc@thread.tacv2","subscribe":{"button":true}}"#
    );
    assert_eq!(
        interacting
            .recalled(&written)
            .and_then(|placed| placed.of::<RouteOptions>().cloned()),
        Some(offered)
    );
}
