use herald_core::{ReadBack, Waits, Wording};
use herald_kinds::{Connect, Effect};
use herald_ports::Method;
use serde_json::{Value, json};

use super::*;
use crate::interact::Replying as Reply;
use crate::interactions::SESSION_DOMAIN;

const APPLICATION: &str = "123456789012345678";
const CHANNEL: &str = "323456789012345678";
const GUILD: &str = "223456789012345678";
const ROLE: &str = "523456789012345678";
const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn options(value: &Value) -> DestinationOptions {
    serde_json::from_value(value.clone()).unwrap()
}

fn bot(interactions: &Value) -> DestinationOptions {
    options(&json!({
        "token": { "secret": "HERALD_DISCORD_TOKEN" },
        "interactions": interactions,
    }))
}

fn webhook(interactions: &Value) -> DestinationOptions {
    options(&json!({
        "webhook": { "secret": "HERALD_DISCORD_WEBHOOK" },
        "interactions": interactions,
    }))
}

fn route(value: &Value) -> RouteOptions {
    serde_json::from_value(value.clone()).unwrap()
}

fn gateway() -> Value {
    json!({ "application": APPLICATION, "via": "gateway", "domains": ["proxy.example"] })
}

fn posted() -> Value {
    json!({ "application": APPLICATION, "key": KEY })
}

fn placing(subscribers: bool) -> Placing<'static> {
    Placing {
        destination: "discord",
        subscribers,
    }
}

fn nothing(_: &str) -> Result<String, String> {
    Err("no file".to_owned())
}

fn secret() -> Secret {
    Secret::new("HERALD_DISCORD_TOKEN", "bot-token")
}

fn messages(faults: &[Fault]) -> Vec<(Option<&str>, &str)> {
    faults
        .iter()
        .map(|fault| (fault.key.as_deref(), fault.message.as_str()))
        .collect()
}

#[test]
fn a_destination_reads_as_a_discord_one_through_its_bot_or_webhook() {
    let written = options(&json!({
        "token": { "secret": "HERALD_DISCORD_TOKEN" },
        "symbols": { "bullet": "*" },
        "waits": { "outage": "7s" },
    }));
    assert_eq!(written.kind(), "discord");
    assert_eq!(written.symbols_written(), written.symbols);
    assert_ne!(written.symbols_written(), Some(Symbols::default()));
    assert_eq!(written.waits_written(), written.waits);
    assert_ne!(written.waits_written(), Some(WaitOptions::default()));
    assert_eq!(
        written.credential().map(|named| named.secret.as_str()),
        Some("HERALD_DISCORD_TOKEN")
    );
    assert_eq!(
        written
            .secret_refs()
            .iter()
            .map(|named| named.secret.as_str())
            .collect::<Vec<_>>(),
        ["HERALD_DISCORD_TOKEN"]
    );
    assert_eq!(written.answering(), None);
    assert_eq!(written.carried(), None);
    assert_eq!(written.unread_why(), "a forum's posts are not read back");

    let hooked = webhook(&posted());
    assert_eq!(
        hooked.credential().map(|named| named.secret.as_str()),
        Some("HERALD_DISCORD_WEBHOOK")
    );
    assert_eq!(hooked.unread_why(), UNREAD);
    assert_eq!(
        hooked.answering(),
        hooked.interactions.as_ref().map(Interactions::answering)
    );
}

#[test]
fn commands_are_posted_unless_the_gateway_carries_them_under_its_domains() {
    let joined = bot(&gateway());
    assert_eq!(joined.session_domains(), [SESSION_DOMAIN, "proxy.example"]);
    assert_eq!(
        joined.carried(),
        Some(Carried {
            posted: false,
            what: "interactions Discord posts",
            session: "the gateway with via = \"gateway\"",
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
fn a_route_reaches_and_profiles_discord_as_its_bot_or_webhook() {
    let forum = route(&json!({ "channel": CHANNEL, "forum": true }));
    let placed = Placed::new(forum.clone());
    let joined = bot(&gateway());
    assert_eq!(
        joined.profile(&placed),
        Some((
            discord::profile(true, true),
            Some(discord::profile(true, true))
        ))
    );
    assert_eq!(
        webhook(&posted()).profile(&placed),
        Some((
            discord::profile(false, true),
            Some(discord::profile(true, true))
        ))
    );
    assert_eq!(
        joined.reach(&placed, "https://herald.example", secret()),
        Some(Reached::new(
            Discord::new(Credential::Bot(secret()), forum.clone(), None)
                .reading_back(ReadBack::DEFAULT)
                .waiting(Waits::DEFAULT)
                .looking(joined.look())
                .worded(Wording::default())
        ))
    );
    assert_eq!(
        joined
            .reach(&placed, "", secret())
            .map(|reached| reached.api().to_owned()),
        Some(crate::discord::API.to_owned())
    );
    assert_eq!(
        webhook(&posted()).reach(&placed, "", secret()),
        Some(Reached::new(Discord::new(
            Credential::Webhook(secret()),
            forum,
            None
        )))
    );
}

#[test]
fn a_route_places_its_channel_threads_each_forum_post_and_offers_its_role() {
    let plain = route(&json!({}));
    assert!(!plain.threads_each_post());
    assert_eq!(plain.place(), None);
    assert_eq!(plain.opt_in_role(), None);
    let forum = route(&json!({ "channel": CHANNEL, "forum": true, "subscribe": { "role": ROLE } }));
    assert!(forum.threads_each_post());
    assert_eq!(forum.place(), Some(CHANNEL));
    assert_eq!(forum.opt_in_role(), Some(ROLE.to_owned()));
}

#[test]
fn a_destination_needs_one_credential_and_a_bot_to_join_the_gateway() {
    assert_eq!(
        DiscordKind::checked(options(&json!({})), &nothing),
        Err(Fault::here(
            "a Discord destination needs a token or a webhook; give one"
        ))
    );
    let (_, faults) = DiscordKind::checked(webhook(&gateway()), &nothing).unwrap();
    assert_eq!(
        messages(&faults),
        [(
            Some("interactions"),
            "the gateway is joined as a bot; give the destination a token in place of its webhook"
        )]
    );
    for sound in [bot(&gateway()), webhook(&posted()), bot(&posted())] {
        let (checked, faults) = DiscordKind::checked(sound.clone(), &nothing).unwrap();
        assert_eq!(checked, sound);
        assert_eq!(faults, []);
    }
}

#[test]
fn a_route_through_a_bot_names_its_channel_and_a_role_needs_a_bot_answering_subscribe() {
    let answering = bot(&gateway());
    assert_eq!(
        messages(&DiscordKind::placed(
            &answering,
            &route(&json!({})),
            placing(false)
        )),
        [(
            None,
            "a Discord bot posts to a channel; give channel = \"<the channel's id>\""
        )]
    );
    assert_eq!(
        DiscordKind::placed(&webhook(&posted()), &route(&json!({})), placing(false)),
        []
    );
    let subscribed = route(&json!({ "channel": CHANNEL, "subscribe": { "role": ROLE } }));
    assert_eq!(
        DiscordKind::placed(&answering, &subscribed, placing(true)),
        []
    );
    assert_eq!(
        messages(&DiscordKind::placed(
            &webhook(&posted()),
            &route(&json!({ "subscribe": { "role": ROLE } })),
            placing(true)
        )),
        [(
            Some("subscribe"),
            "a member is given the opt-in role by a bot; give the destination a token in place of its webhook"
        )]
    );
    assert_eq!(
        messages(&DiscordKind::placed(
            &answering,
            &route(&json!({ "channel": "general" })),
            placing(false)
        )),
        [(
            None,
            "\"general\" is not a channel id; give the channel's id, 17 to 20 digits, from Copy Channel ID"
        )]
    );
}

fn interacting(interactions: &Value) -> Interacting {
    Interacting::new(
        serde_json::from_value(interactions.clone()).unwrap(),
        Some(secret()),
        Some("https://discord.test/api/"),
    )
}

fn reply() -> Reply {
    Reply {
        id: "900".to_owned(),
        token: Secret::new("interaction", "interaction-token"),
    }
}

fn asked(interacting: &Interacting) -> Asked {
    let interaction = json!({
        "id": "900",
        "token": "interaction-token",
        "type": 2,
        "application_id": APPLICATION,
        "channel_id": CHANNEL,
        "guild_id": GUILD,
        "member": { "user": { "id": "423456789012345678" }, "roles": [] },
        "data": { "name": "latest" },
    });
    match interacting.received(
        &DestinationName::new("discord"),
        interaction.to_string().as_bytes(),
    ) {
        Ok(Received::Asked { asked, .. }) => asked,
        other => panic!("{other:?}"),
    }
}

fn response(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

#[test]
fn commands_are_answered_through_the_interaction_they_came_with() {
    let interacting = interacting(&posted());
    let replying = Replying::new(reply());
    let parts = [
        Text {
            body: "one".to_owned(),
            html: None,
        },
        Text {
            body: "two".to_owned(),
            html: None,
        },
    ];
    assert_eq!(Interacts::api(&interacting), "https://discord.test/api");
    assert_eq!(
        Interacts::answering(&interacting),
        Interacting::answering(&interacting)
    );
    assert_eq!(
        Interacts::answered(&interacting, &replying, &parts, true),
        Interacting::answered(
            &interacting,
            &reply(),
            &["one".to_owned(), "two".to_owned()],
            true
        )
    );
    assert_eq!(
        Interacts::answered(&interacting, &Replying::new(()), &parts, true),
        []
    );
    assert_eq!(
        Interacts::took(&interacting, &response(400, &json!({ "message": "no" }))),
        Err("Discord answered 400: no".to_owned())
    );
    assert_eq!(
        Interacts::took(&interacting, &response(204, &Value::Null)),
        Ok(())
    );
    let asked = asked(&interacting);
    let public = Interacting::answering(&interacting)
        .replies
        .public(asked.command, asked.share);
    assert_eq!(
        interacting.deferred(&replying, &asked),
        Some(interacting.defer(&reply(), public))
    );
    assert_eq!(interacting.deferred(&Replying::new(()), &asked), None);
    let projects = ["herald".to_owned()];
    assert_eq!(
        Interacts::choices(&interacting, &projects),
        choices(&projects)
    );
    assert_eq!(
        Interacts::complete(&interacting, &replying, &projects),
        Some(Interacting::complete(&interacting, &reply(), &projects))
    );
    assert!(interacting.gives_roles());
    let given = Interacts::role(&interacting, GUILD, "423456789012345678", ROLE, true).unwrap();
    assert_eq!(given.method, Method::Put);
}

#[test]
fn interactions_are_posted_under_the_key_they_are_signed_with() {
    let interacting = interacting(&posted());
    assert_eq!(
        interacting.posted(Endpoint::Posted),
        Some(Ok((Scheme::Discord, KEY)))
    );
    assert_eq!(interacting.posted(Endpoint::Pushed), None);
    assert_eq!(
        self::interacting(&gateway()).posted(Endpoint::Posted),
        Some(Err("interactions for such a destination"))
    );
    assert!(interacting.session().is_none());
}

#[test]
fn the_gateway_is_joined_as_the_bot_and_heard_as_herald_answers_it() {
    let interacting = interacting(&gateway());
    let mut session = interacting.session().unwrap();
    assert_eq!(
        session.connect(),
        Connect::Socket(crate::GATEWAY.to_owned())
    );
    assert_eq!(
        session.unreachable(),
        "Discord's gateway could not be reached"
    );
    let heard = session.heard(
        &DestinationName::new("discord"),
        &json!({ "op": 10, "d": { "heartbeat_interval": 41250 } }).to_string(),
    );
    assert_eq!(
        heard.first(),
        Some(&Effect::Beat(jiff::SignedDuration::from_millis(41250)))
    );
    assert!(matches!(heard.get(1), Some(Effect::Send(_))), "{heard:?}");
    assert!(matches!(session.beat().as_slice(), [Effect::Send(_)]));
    assert_eq!(session.closed(Some(4004)).len(), 1);
    let webhooked = Interacting::new(serde_json::from_value(gateway()).unwrap(), None, None);
    assert!(webhooked.session().is_none());
}
