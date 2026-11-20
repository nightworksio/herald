//! Who may view a Discord route's channel, read right before a run's first
//! advisories section is sent there (ADR-0067).

use herald_core::Delivery;
use herald_dest_discord::{Credential, Discord, RouteOptions, UNCHECKED};
use herald_kinds::{Audience, Probe, Reach};
use herald_ports::{Response, Secret};
use herald_testing::ScriptedHttp;
use serde_json::{Value, json};

const CHANNEL: &str = "1566184281406177531";
const GUILD: &str = "1566184281406170000";
const MODS: &str = "1566184281406170001";
const ADMINS: &str = "1566184281406170002";
const MEMBER: &str = "1566184281406170003";
const VIEW: u64 = 1 << 10;
const ADMINISTRATOR: u64 = 1 << 3;

fn bot(channel: &str, viewers: &[&str]) -> Discord {
    Discord::new(
        Credential::Bot(Secret::new("HERALD_DISCORD_TOKEN", "test-token")),
        RouteOptions {
            channel: Some(channel.to_owned()),
            forum: false,
            subscribe: None,
            viewers: viewers.iter().map(|viewer| (*viewer).to_owned()).collect(),
        },
        None,
    )
}

fn answer(status: u16, body: &Value) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.to_string().into_bytes(),
    }
}

fn overwrite(id: &str, kind: u8, (allow, deny): (u64, u64)) -> Value {
    json!({ "id": id, "type": kind, "allow": allow.to_string(), "deny": deny.to_string() })
}

fn channel(kind: u8, overwrites: &[Value]) -> Value {
    json!({ "id": CHANNEL, "type": kind, "guild_id": GUILD, "permission_overwrites": overwrites })
}

fn hidden(more: &[Value]) -> Value {
    let mut overwrites = vec![overwrite(GUILD, 0, (0, VIEW))];
    overwrites.extend_from_slice(more);
    channel(0, &overwrites)
}

fn server(everyone: u64, roles: &[(&str, &str, u64)]) -> Value {
    let mut listed =
        vec![json!({ "id": GUILD, "name": "@everyone", "permissions": everyone.to_string() })];
    listed.extend(roles.iter().map(|(id, name, permissions)| {
        json!({ "id": id, "name": name, "permissions": permissions.to_string() })
    }));
    json!({ "id": GUILD, "roles": listed })
}

fn onboarding(channels: &[&str], roles: &[&str]) -> Value {
    json!({ "guild_id": GUILD, "enabled": true, "default_channel_ids": channels,
        "prompts": [{ "options": [{ "channel_ids": [], "role_ids": roles }] }] })
}

fn audience(discord: &Discord, answers: Vec<Response>) -> (Audience, Vec<String>) {
    let http = ScriptedHttp::new(answers);
    let audience = pollster::block_on(discord.audience(&http));
    let asked = http.asked().into_iter().map(|(url, _)| url).collect();
    (audience, asked)
}

fn checked(viewers: &[&str], (channel, server, onboarding): (Value, Value, Value)) -> Audience {
    let answers = vec![
        answer(200, &channel),
        answer(200, &server),
        answer(200, &onboarding),
    ];
    audience(&bot(CHANNEL, viewers), answers).0
}

#[test]
fn a_channel_everyone_in_its_server_may_view_is_public_and_one_denied_to_them_private() {
    let (private, asked) = audience(
        &bot(CHANNEL, &[]),
        vec![
            answer(200, &hidden(&[])),
            answer(200, &server(VIEW, &[(MODS, "Mods", 0)])),
            answer(200, &onboarding(&[], &[])),
        ],
    );
    assert_eq!(private, Audience::Private);
    assert_eq!(
        asked,
        [
            format!("https://discord.com/api/v10/channels/{CHANNEL}"),
            format!("https://discord.com/api/v10/guilds/{GUILD}"),
            format!("https://discord.com/api/v10/guilds/{GUILD}/onboarding"),
        ]
    );
    for (overwrites, everyone) in [
        (Vec::new(), VIEW),
        (vec![overwrite(GUILD, 0, (VIEW, 0))], 0),
        (vec![overwrite(GUILD, 0, (0, VIEW))], ADMINISTRATOR),
        (vec![overwrite(GUILD, 0, (VIEW, VIEW))], 0),
    ] {
        let (public, asked) = audience(
            &bot(CHANNEL, &[]),
            vec![
                answer(200, &channel(0, &overwrites)),
                answer(200, &server(everyone, &[])),
            ],
        );
        assert!(
            matches!(public, Audience::Public(_)),
            "{overwrites:?} {everyone}"
        );
        assert_eq!(asked.len(), 2);
    }
}

#[test]
fn every_role_and_member_that_may_view_the_channel_must_be_one_the_route_lists() {
    let mods = || hidden(&[overwrite(MODS, 0, (VIEW, 0))]);
    let roles = [(MODS, "Mods", 0), (ADMINS, "Admins", ADMINISTRATOR)];
    let unlisted = checked(&[], (mods(), server(VIEW, &roles), onboarding(&[], &[])));
    assert_eq!(
        unlisted,
        Audience::Public(format!(
            "the role Mods ({MODS}), the role Admins ({ADMINS}) may view the channel and the route's viewers does not list them"
        ))
    );
    let half = checked(
        &[MODS],
        (mods(), server(VIEW, &roles), onboarding(&[], &[])),
    );
    assert!(
        matches!(&half, Audience::Public(why) if why.starts_with("the role Admins")),
        "{half:?}"
    );
    let listed = checked(
        &[MODS, ADMINS],
        (mods(), server(VIEW, &roles), onboarding(&[], &[])),
    );
    assert_eq!(listed, Audience::Private);
    let denied = hidden(&[overwrite(MODS, 0, (0, VIEW))]);
    let own = checked(
        &[],
        (
            denied,
            server(VIEW, &[(MODS, "Mods", VIEW)]),
            onboarding(&[], &[]),
        ),
    );
    assert_eq!(own, Audience::Private);
    let member = || hidden(&[overwrite(MEMBER, 1, (VIEW, 0))]);
    let let_in = checked(&[], (member(), server(VIEW, &[]), onboarding(&[], &[])));
    assert_eq!(
        let_in,
        Audience::Public(format!(
            "the member {MEMBER} may view the channel and the route's viewers does not list them"
        ))
    );
    let named = checked(
        &[MEMBER],
        (member(), server(VIEW, &[]), onboarding(&[], &[])),
    );
    assert_eq!(named, Audience::Private);
}

#[test]
fn a_channel_or_role_the_server_s_onboarding_hands_out_is_never_private() {
    let mods = || hidden(&[overwrite(MODS, 0, (VIEW, 0))]);
    let roles = [(MODS, "Mods", 0)];
    let shown = checked(
        &[MODS],
        (mods(), server(VIEW, &roles), onboarding(&[CHANNEL], &[])),
    );
    assert_eq!(
        shown,
        Audience::Public("the server's onboarding shows the channel to whoever joins".to_owned())
    );
    let handed = checked(
        &[MODS],
        (mods(), server(VIEW, &roles), onboarding(&[], &[MODS])),
    );
    assert_eq!(
        handed,
        Audience::Public(format!(
            "the server's onboarding lets whoever joins take the role Mods ({MODS}), which may view the channel"
        ))
    );
    let (unread, _) = audience(
        &bot(CHANNEL, &[MODS]),
        vec![
            answer(200, &mods()),
            answer(200, &server(VIEW, &roles)),
            answer(403, &json!({})),
        ],
    );
    assert!(matches!(unread, Audience::Unread(_)), "{unread:?}");
}

#[test]
fn a_channel_herald_cannot_read_is_never_taken_as_private() {
    let unread = |discord: &Discord, answers: Vec<Response>| {
        let (audience, _) = audience(discord, answers);
        assert!(matches!(audience, Audience::Unread(_)), "{audience:?}");
    };
    let webhook = Discord::new(
        Credential::Webhook(Secret::new(
            "HERALD_DISCORD_WEBHOOK",
            "https://discord.com/api/webhooks/1/x",
        )),
        RouteOptions::default(),
        None,
    );
    let (refused, asked) = audience(&webhook, Vec::new());
    assert_eq!(
        (refused, asked),
        (Audience::Unread(UNCHECKED.to_owned()), Vec::<String>::new())
    );
    unread(&bot("..", &[]), Vec::new());
    unread(&bot(CHANNEL, &[]), vec![answer(403, &json!({}))]);
    unread(
        &bot(CHANNEL, &[]),
        vec![answer(200, &json!({ "type": "x" }))],
    );
    unread(&bot(CHANNEL, &[]), vec![answer(200, &channel(11, &[]))]);
    unread(
        &bot(CHANNEL, &[]),
        vec![answer(200, &json!({ "id": CHANNEL, "type": 0 }))],
    );
    unread(
        &bot(CHANNEL, &[]),
        vec![answer(200, &channel(0, &[])), answer(404, &json!({}))],
    );
    unread(
        &bot(CHANNEL, &[]),
        vec![
            answer(200, &channel(0, &[])),
            answer(200, &json!({ "roles": [] })),
        ],
    );
    let odd = json!({ "type": 0, "guild_id": GUILD, "permission_overwrites": [{ "id": GUILD, "type": 0, "allow": "x", "deny": "0" }] });
    unread(
        &bot(CHANNEL, &[]),
        vec![answer(200, &odd), answer(200, &server(0, &[]))],
    );
    let odd_role = json!({ "id": GUILD, "roles": [{ "id": GUILD, "name": "@everyone", "permissions": "0" }, { "id": MODS, "name": "Mods", "permissions": "x" }] });
    unread(
        &bot(CHANNEL, &[]),
        vec![answer(200, &hidden(&[])), answer(200, &odd_role)],
    );
    unread(&bot(CHANNEL, &[]), Vec::new());
    assert!(!bot(CHANNEL, &[]).keeps_edits());
}

#[test]
fn doctor_names_the_roles_that_withhold_a_channel_its_section() {
    let http = ScriptedHttp::new(vec![
        answer(200, &json!({ "id": "1" })),
        answer(200, &hidden(&[overwrite(MODS, 0, (VIEW, 0))])),
        answer(200, &server(VIEW, &[(MODS, "Mods", 0)])),
    ]);
    let probes = pollster::block_on(bot(CHANNEL, &[]).probe(&http, &Delivery::default()));
    assert_eq!(
        probes,
        [
            Probe::Taken,
            Probe::Public(format!(
                "the role Mods ({MODS}) may view the channel and the route's viewers does not list them"
            )),
        ]
    );
}
