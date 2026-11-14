use herald_core::{Asked, Command, DestinationName, Member, Place, Replies};
use serde_json::json;

use super::*;
use crate::SESSION_DOMAIN;

const APPLICATION: &str = "123456789012345678";
const GUILD: &str = "223456789012345678";
const CHANNEL: &str = "323456789012345678";
const USER: &str = "423456789012345678";
const ROLE: &str = "523456789012345678";
const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn interactions(extra: &str) -> Interactions {
    serde_json::from_str(&format!(
        r#"{{"application": "{APPLICATION}", "key": "{KEY}"{extra}}}"#
    ))
    .unwrap()
}

fn bot() -> Secret {
    Secret::new("HERALD_DISCORD_TOKEN", "bot-token")
}

fn interacting(extra: &str) -> Interacting {
    Interacting::new(
        interactions(extra),
        Some(bot()),
        Some("https://discord.test/api/"),
    )
}

fn destination() -> DestinationName {
    DestinationName::new("discord")
}

fn interaction(kind: u64, data: &Value) -> Value {
    json!({
        "id": "900",
        "token": "interaction-token",
        "type": kind,
        "application_id": APPLICATION,
        "channel_id": CHANNEL,
        "guild_id": GUILD,
        "member": { "user": { "id": USER }, "roles": [ROLE, "623456789012345678"] },
        "data": data,
    })
}

fn received(interacting: &Interacting, value: &Value) -> Result<Received, UnreadInteraction> {
    interacting.received(&destination(), value.to_string().as_bytes())
}

fn taken(received: Result<Received, UnreadInteraction>) -> (Asked, Replying) {
    match received {
        Ok(Received::Asked { asked, replying }) => (asked, replying),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_command_in_a_thread_is_placed_in_the_channel_the_thread_is_in_too() {
    const FORUM: &str = "723456789012345678";
    let in_channel = |kind: u64, parent: &str| {
        let mut value = interaction(2, &json!({ "name": "latest" }));
        value.as_object_mut().unwrap().insert(
            "channel".to_owned(),
            json!({ "id": CHANNEL, "type": kind, "parent_id": parent }),
        );
        taken(received(&interacting(""), &value)).0.place
    };
    for thread in [10, 11, 12] {
        assert_eq!(
            in_channel(thread, FORUM),
            Place {
                id: CHANNEL.to_owned(),
                parent: Some(FORUM.to_owned()),
            },
            "{thread}"
        );
    }
    assert_eq!(in_channel(0, FORUM), Place::new(CHANNEL));
    assert_eq!(in_channel(11, "forum"), Place::new(CHANNEL));
}

#[test]
fn a_ping_is_read_whoever_sent_it_and_answered_with_a_pong() {
    let ping = json!({ "type": 1 });
    assert_eq!(received(&interacting(""), &ping), Ok(Received::Ping));
    assert_eq!(pong(), r#"{"type":1}"#);
}

#[test]
fn a_slash_command_is_read_with_its_project_share_and_the_member_s_roles() {
    let value = interaction(
        2,
        &json!({ "name": "latest", "options": [
            { "name": "project", "type": 3, "value": "herald" },
            { "name": "share", "type": 5, "value": true },
        ]}),
    );
    let (asked, replying) = taken(received(&interacting(""), &value));
    assert_eq!(
        asked,
        Asked {
            destination: destination(),
            place: Place::new(CHANNEL),
            user: USER.to_owned(),
            command: Command::Latest,
            project: Some("herald".to_owned()),
            share: true,
            toggles: false,
            member: Some(Member {
                guild: GUILD.to_owned(),
                roles: vec![ROLE.to_owned(), "623456789012345678".to_owned()],
            }),
        }
    );
    assert_eq!(replying.id, "900");
    assert_eq!(replying.token.expose(), "interaction-token");
}

#[test]
fn a_command_given_with_no_options_names_no_project_and_shares_nothing() {
    let value = interaction(2, &json!({ "name": "subscribe" }));
    let (asked, _) = taken(received(&interacting(""), &value));
    assert_eq!(asked.command, Command::Subscribe);
    assert_eq!(asked.project, None);
    assert!(!asked.share);
}

#[test]
fn a_subcommand_of_the_group_is_read_and_a_command_outside_it_ignored() {
    let grouped = interacting(r#", "group": "herald""#);
    let value = interaction(
        2,
        &json!({ "name": "herald", "options": [
            { "name": "changelog", "type": 1, "options": [
                { "name": "project", "type": 3, "value": "gate" },
            ]},
        ]}),
    );
    let (asked, _) = taken(received(&grouped, &value));
    assert_eq!(asked.command, Command::Changelog);
    assert_eq!(asked.project.as_deref(), Some("gate"));
    let outside = interaction(2, &json!({ "name": "latest" }));
    assert_eq!(received(&grouped, &outside), Ok(Received::Ignored));
    let another = interaction(
        2,
        &json!({ "name": "other", "options": [
            { "name": "latest", "type": 1 },
        ]}),
    );
    assert_eq!(received(&grouped, &another), Ok(Received::Ignored));
    let empty = interaction(2, &json!({ "name": "herald", "options": [] }));
    assert_eq!(received(&grouped, &empty), Ok(Received::Ignored));
    let unknown = interaction(
        2,
        &json!({ "name": "herald", "options": [
            { "name": "deploy", "type": 1 },
        ]}),
    );
    assert_eq!(received(&grouped, &unknown), Ok(Received::Ignored));
}

#[test]
fn typing_a_project_is_read_as_what_was_typed_in_its_channel() {
    let value = interaction(
        4,
        &json!({ "name": "latest", "options": [
            { "name": "share", "type": 5, "value": false },
            { "name": "project", "type": 3, "value": "her", "focused": true },
        ]}),
    );
    let replying = Replying {
        id: "900".to_owned(),
        token: Secret::new("the interaction's token", "interaction-token"),
    };
    assert_eq!(
        received(&interacting(""), &value),
        Ok(Received::Completing {
            place: Place::new(CHANNEL),
            typed: "her".to_owned(),
            replying: replying.clone(),
        })
    );
    let nothing = interaction(4, &json!({ "name": "latest", "options": [] }));
    assert_eq!(
        received(&interacting(""), &nothing),
        Ok(Received::Completing {
            place: Place::new(CHANNEL),
            typed: String::new(),
            replying: replying.clone(),
        })
    );
    let mut tokenless_typing = interaction(4, &json!({ "name": "latest" }));
    tokenless_typing.as_object_mut().unwrap().remove("token");
    assert_eq!(
        received(&interacting(""), &tokenless_typing),
        Err(UnreadInteraction::NotAnInteraction)
    );
    let mut idless = interaction(4, &json!({ "name": "latest" }));
    idless.as_object_mut().unwrap().remove("id");
    assert_eq!(
        received(&interacting(""), &idless),
        Err(UnreadInteraction::NotAnInteraction)
    );
    let completing = interacting("").complete(&replying, &["herald".to_owned()]);
    assert_eq!(
        completing.url.expose(),
        "https://discord.test/api/interactions/900/interaction-token/callback"
    );
    assert_eq!(
        completing.body,
        choices(&["herald".to_owned()]).into_bytes()
    );
}

#[test]
fn a_subscribe_button_toggles_and_another_button_is_ignored() {
    let value = interaction(
        3,
        &json!({ "custom_id": "herald:subscribe:herald", "component_type": 2 }),
    );
    let (asked, _) = taken(received(&interacting(""), &value));
    assert_eq!(asked.command, Command::Subscribe);
    assert_eq!(asked.project.as_deref(), Some("herald"));
    assert!(asked.toggles);
    let other = interaction(3, &json!({ "custom_id": "someone:else" }));
    assert_eq!(received(&interacting(""), &other), Ok(Received::Ignored));
    assert_eq!(
        subscribe_button("herald").as_deref(),
        Some("herald:subscribe:herald")
    );
    assert_eq!(
        subscribe_button(&"p".repeat(83)).map(|id| id.len()),
        Some(100)
    );
    assert_eq!(subscribe_button(&"p".repeat(84)), None);
}

#[test]
fn another_application_s_interaction_and_unknown_kinds_are_ignored() {
    let mut value = interaction(2, &json!({ "name": "latest" }));
    value["application_id"] = json!("999999999999999999");
    assert_eq!(received(&interacting(""), &value), Ok(Received::Ignored));
    let modal = interaction(5, &json!({}));
    assert_eq!(received(&interacting(""), &modal), Ok(Received::Ignored));
    let unknown = interaction(2, &json!({ "name": "deploy" }));
    assert_eq!(received(&interacting(""), &unknown), Ok(Received::Ignored));
}

#[test]
fn a_command_in_a_direct_message_is_read_with_no_member() {
    let mut value = interaction(2, &json!({ "name": "latest" }));
    let object = value.as_object_mut().unwrap();
    object.remove("member");
    object.remove("guild_id");
    object.insert("user".to_owned(), json!({ "id": USER }));
    let (asked, _) = taken(received(&interacting(""), &value));
    assert_eq!(asked.user, USER);
    assert_eq!(asked.member, None);
    let mut roleless = interaction(2, &json!({ "name": "latest" }));
    roleless["member"] = json!({ "user": { "id": USER } });
    let (asked, _) = taken(received(&interacting(""), &roleless));
    assert_eq!(asked.member.map(|member| member.roles), Some(Vec::new()));
}

#[test]
fn an_interaction_missing_what_it_needs_is_unread() {
    assert_eq!(
        interacting("").received(&destination(), b"not json"),
        Err(UnreadInteraction::NotAnInteraction)
    );
    assert_eq!(
        received(&interacting(""), &json!({ "id": "1" })),
        Err(UnreadInteraction::NotAnInteraction)
    );
    let mut dataless = interaction(2, &json!({}));
    dataless.as_object_mut().unwrap().remove("data");
    assert_eq!(
        received(&interacting(""), &dataless),
        Err(UnreadInteraction::NotAnInteraction)
    );
    let mut unplaced = interaction(2, &json!({ "name": "latest" }));
    unplaced.as_object_mut().unwrap().remove("channel_id");
    assert_eq!(
        received(&interacting(""), &unplaced),
        Err(UnreadInteraction::Unplaced)
    );
    let mut userless = interaction(2, &json!({ "name": "latest" }));
    userless.as_object_mut().unwrap().remove("member");
    assert_eq!(
        received(&interacting(""), &userless),
        Err(UnreadInteraction::Unplaced)
    );
    for (key, unwritten) in [
        ("/channel_id", ""),
        ("/channel_id", "general"),
        ("/member/user/id", ""),
    ] {
        let mut miswritten = interaction(2, &json!({ "name": "latest" }));
        *miswritten.pointer_mut(key).unwrap() = json!(unwritten);
        assert_eq!(
            received(&interacting(""), &miswritten),
            Err(UnreadInteraction::Unplaced),
            "{key} {unwritten:?}"
        );
    }
    for key in ["id", "token"] {
        let mut missing = interaction(2, &json!({ "name": "latest" }));
        missing.as_object_mut().unwrap().remove(key);
        assert_eq!(
            received(&interacting(""), &missing),
            Err(UnreadInteraction::NotAnInteraction),
            "{key}"
        );
    }
    assert_eq!(
        UnreadInteraction::NotAnInteraction.to_string(),
        "the body is not a Discord interaction"
    );
    assert_eq!(
        UnreadInteraction::Unplaced.to_string(),
        "the interaction names no channel or no member"
    );
}

#[test]
fn the_commands_are_registered_on_their_own_or_as_the_group_s_subcommands() {
    let registered = interacting(r#", "commands": ["subscribe", "latest"]"#).commands();
    assert_eq!(registered.as_array().map(Vec::len), Some(2));
    assert_eq!(registered[0]["name"], "subscribe");
    assert_eq!(registered[0]["options"].as_array().map(Vec::len), Some(1));
    assert_eq!(registered[0]["options"][0]["autocomplete"], true);
    assert_eq!(registered[1]["name"], "latest");
    assert_eq!(registered[1]["options"][1]["name"], "share");
    assert_eq!(
        registered[1]["description"],
        herald_core::Wording::default().describe(Command::Latest)
    );
    let dutch = interacting(
        r#", "group": "herald", "wording": {"describe_latest": "Toon de laatste", "describe_group": "Vraag herald", "describe_project": "Het project", "describe_share": "Toon iedereen"}"#,
    )
    .commands();
    assert_eq!(dutch[0]["description"], "Vraag herald");
    let latest = &dutch[0]["options"][2];
    assert_eq!(latest["description"], "Toon de laatste");
    assert_eq!(latest["options"][0]["description"], "Het project");
    assert_eq!(latest["options"][1]["description"], "Toon iedereen");
    let grouped = interacting(r#", "group": "herald""#).commands();
    assert_eq!(grouped[0]["name"], "herald");
    assert_eq!(grouped[0]["options"].as_array().map(Vec::len), Some(4));
    assert_eq!(grouped[0]["options"][2]["type"], 1);
    assert_eq!(grouped[0]["options"][2]["name"], "latest");
}

#[test]
fn registering_puts_every_command_globally_or_in_the_server_named() {
    let global = interacting("").register().unwrap();
    assert_eq!(global.method, Method::Put);
    assert_eq!(
        global.url.expose(),
        format!("https://discord.test/api/applications/{APPLICATION}/commands")
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&global.body).unwrap(),
        interacting("").commands()
    );
    let guild = interacting(&format!(r#", "guild": "{GUILD}""#))
        .register()
        .unwrap();
    assert_eq!(
        guild.url.expose(),
        format!("https://discord.test/api/applications/{APPLICATION}/guilds/{GUILD}/commands")
    );
    let botless = Interacting::new(interactions(""), None, None);
    assert_eq!(botless.register(), None);
    assert_eq!(botless.api(), "https://discord.com/api/v10");
    assert_eq!(botless.bot(), None);
    assert_eq!(interacting("").bot(), Some(&bot()));
}

#[test]
fn a_role_is_given_and_taken_by_the_bot_with_a_reason_and_ids_checked() {
    let given = interacting("").role(GUILD, USER, ROLE, true).unwrap();
    assert_eq!(given.method, Method::Put);
    assert_eq!(
        given.url.expose(),
        format!("https://discord.test/api/guilds/{GUILD}/members/{USER}/roles/{ROLE}")
    );
    let header = |request: &Request, name: &str| {
        request
            .headers
            .iter()
            .find(|(header, _)| header == name)
            .map(|(_, value)| value.expose())
    };
    assert_eq!(
        header(&given, "Authorization").as_deref(),
        Some("Bot bot-token")
    );
    assert_eq!(
        header(&given, "X-Audit-Log-Reason").as_deref(),
        Some("Subscribed through herald")
    );
    let taken = interacting("").role(GUILD, USER, ROLE, false).unwrap();
    assert_eq!(taken.method, Method::Delete);
    assert_eq!(
        header(&taken, "X-Audit-Log-Reason").as_deref(),
        Some("Unsubscribed through herald")
    );
    let worded = interacting(r#", "wording": {"audit_subscribed": "Via herald aangemeld"}"#)
        .role(GUILD, USER, ROLE, true)
        .unwrap();
    assert_eq!(
        header(&worded, "X-Audit-Log-Reason").as_deref(),
        Some("Via herald aangemeld")
    );
    assert_eq!(interacting("").role("../x", USER, ROLE, true), None);
    assert_eq!(interacting("").role(GUILD, "1", ROLE, true), None);
    assert_eq!(interacting("").role(GUILD, USER, "r", true), None);
    assert_eq!(
        Interacting::new(interactions(""), None, None).role(GUILD, USER, ROLE, true),
        None
    );
}

#[test]
fn roles_are_listed_and_made_mentionable_in_a_server_named_by_id() {
    let listing = interacting("").roles(GUILD).unwrap();
    assert_eq!(listing.method, Method::Get);
    assert_eq!(
        listing.url.expose(),
        format!("https://discord.test/api/guilds/{GUILD}/roles")
    );
    assert_eq!(interacting("").roles("x"), None);
    let making = interacting("").make_role(GUILD, "herald updates").unwrap();
    assert_eq!(making.method, Method::Post);
    assert_eq!(
        serde_json::from_slice::<Value>(&making.body).unwrap(),
        json!({ "name": "herald updates", "mentionable": true })
    );
    assert_eq!(interacting("").make_role("x", "n"), None);
    let answer = |status, body: &str| Response {
        status,
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
    };
    assert_eq!(
        listed(&answer(200, r#"[{"id": "1", "name": "a"}, {"id": "2"}]"#)),
        Ok(vec![("1".to_owned(), "a".to_owned())])
    );
    assert_eq!(listed(&answer(200, "{}")), Ok(Vec::new()));
    assert_eq!(
        listed(&answer(403, r#"{"message": "Missing Permissions"}"#)),
        Err("Discord refused it with 403: Missing Permissions; check what herald's credential is allowed to do there".to_owned())
    );
    assert_eq!(accepted(&answer(204, "")), Ok(()));
    assert_eq!(
        made(&answer(200, r#"{"id": "9", "name": "updates"}"#)),
        Ok(("9".to_owned(), "updates".to_owned()))
    );
    assert_eq!(
        made(&answer(200, "{}")),
        Err("Discord answered with no role".to_owned())
    );
    assert_eq!(
        made(&answer(403, "")),
        Err(
            "Discord refused it with 403; check what herald's credential is allowed to do there"
                .to_owned()
        )
    );
    assert_eq!(
        accepted(&answer(500, "")),
        Err("Discord refused it with 500".to_owned())
    );
    assert_eq!(
        accepted(&answer(
            429,
            r#"{"message":"You are being rate limited.","retry_after":2.5}"#
        )),
        Err(
            "Discord refused it with 429: You are being rate limited., and asked to wait 2s 500ms; herald sent more than the platform takes at once"
                .to_owned()
        )
    );
}

#[test]
fn an_answer_is_deferred_then_edited_into_place_with_the_rest_followed_up() {
    let interacting = interacting("");
    let replying = Replying {
        id: "900".to_owned(),
        token: Secret::new("the interaction's token", "tok"),
    };
    let defer = interacting.defer(&replying, false);
    assert_eq!(defer.method, Method::Post);
    assert_eq!(
        defer.url.expose(),
        "https://discord.test/api/interactions/900/tok/callback"
    );
    assert!(defer.url.redacted().contains("the interaction's token"));
    assert_eq!(defer.body, deferred(false).into_bytes());
    let parts = ["one".to_owned(), "two".to_owned()];
    let private = interacting.answered(&replying, &parts, false);
    assert_eq!(private.len(), 2);
    assert_eq!(private[0].method, Method::Patch);
    assert_eq!(
        private[0].url.expose(),
        format!("https://discord.test/api/webhooks/{APPLICATION}/tok/messages/@original")
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&private[0].body).unwrap(),
        json!({ "content": "one", "allowed_mentions": { "parse": [] } })
    );
    assert_eq!(private[1].method, Method::Post);
    assert_eq!(
        private[1].url.expose(),
        format!("https://discord.test/api/webhooks/{APPLICATION}/tok")
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&private[1].body).unwrap(),
        json!({ "content": "two", "flags": 64, "allowed_mentions": { "parse": [] } })
    );
    let public = interacting.answered(&replying, &parts, true);
    assert_eq!(
        serde_json::from_slice::<Value>(&public[1].body).unwrap()["flags"],
        0
    );
}

#[test]
fn responses_say_working_privately_or_publicly_and_list_choices() {
    assert_eq!(deferred(false), r#"{"data":{"flags":64},"type":5}"#);
    assert_eq!(deferred(true), r#"{"type":5}"#);
    assert_eq!(
        choices(&["gate".to_owned()]),
        r#"{"data":{"choices":[{"name":"gate","value":"gate"}]},"type":8}"#
    );
}

#[test]
fn the_destination_says_how_it_carries_and_answers_commands() {
    assert_eq!(interacting(r#", "via": "gateway""#).via(), Via::Gateway);
    assert_eq!(
        interacting(r#", "domains": ["proxy.example"]"#).session_domains(),
        [SESSION_DOMAIN, "proxy.example"]
    );
    assert_eq!(interacting(r#", "os": "freebsd""#).os(), Some("freebsd"));
    let interacting = interacting(r#", "replies": "public""#);
    assert_eq!(interacting.os(), None);
    assert_eq!(interacting.via(), Via::Http);
    assert_eq!(interacting.key(), Some(KEY));
    assert_eq!(interacting.answering().replies, Replies::Public);
}

#[test]
fn an_answer_to_a_command_of_another_kind_is_no_request() {
    let foreign = herald_kinds::Replying::new(0_u8);
    assert_eq!(
        herald_kinds::Interacts::answered(&interacting(""), &foreign, &[], false),
        Vec::new()
    );
}
