//! What doctor `--online` says of the permissions a Discord bot holds in
//! its route's channel, against what the route's delivery needs
//! (ADR-0002, ADR-0004).

use herald_core::{Delivery, Edits, Mentions, Pinning, RoleId, Thread};
use herald_dest_discord::{Credential, Discord, RouteOptions, Subscribe};
use herald_kinds::{Probe, Reach, Recorded};
use herald_ports::{Response, Secret};
use herald_testing::ScriptedHttp;
use jiff::SignedDuration;
use serde_json::{Value, json};

const BOT: &str = "1566184281406179999";
const CHANNEL: &str = "1566184281406177531";
const GUILD: &str = "1566184281406170000";
const HERALD: &str = "1566184281406170001";
const NEWS: &str = "1566184281406170002";
const QUIET: &str = "1566184281406170003";
const OTHER: &str = "1566184281406170004";
const VIEW: u64 = 1 << 10;
const SEND: u64 = 1 << 11;
const READ_HISTORY: u64 = 1 << 16;
const MENTION_EVERYONE: u64 = 1 << 17;
const MANAGE_ROLES: u64 = 1 << 28;
const ADMINISTRATOR: u64 = 1 << 3;
const ROLE: u8 = 0;
const MEMBER: u8 = 1;
const FORUM: u8 = 15;
const THREAD: u8 = 11;

fn discord(forum: bool, subscribe: Option<&str>) -> Discord {
    Discord::new(
        Credential::Bot(Secret::new("HERALD_DISCORD_TOKEN", "test-token")),
        RouteOptions {
            channel: Some(CHANNEL.to_owned()),
            forum,
            subscribe: subscribe.map(|role| Subscribe {
                role: role.to_owned(),
                button: true,
            }),
            viewers: vec![HERALD.to_owned()],
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
    let mut listed = vec![overwrite(GUILD, ROLE, (0, VIEW))];
    listed.extend_from_slice(overwrites);
    json!({ "id": CHANNEL, "type": kind, "guild_id": GUILD, "permission_overwrites": listed })
}

fn role(id: &str, name: &str, permissions: u64, (position, mentionable): (i64, bool)) -> Value {
    json!({ "id": id, "name": name, "permissions": permissions.to_string(),
        "position": position, "mentionable": mentionable })
}

fn server(owner: &str, roles: &[Value]) -> Value {
    let mut listed = vec![role(GUILD, "@everyone", VIEW | SEND, (0, false))];
    listed.extend_from_slice(roles);
    json!({ "id": GUILD, "owner_id": owner, "roles": listed })
}

fn roles() -> Vec<Value> {
    vec![
        role(HERALD, "Herald", 0, (2, false)),
        role(NEWS, "News", 0, (1, true)),
        role(QUIET, "Quiet", 0, (5, false)),
    ]
}

fn held(roles: &[&str]) -> Value {
    json!({ "roles": roles })
}

fn onboarding() -> Value {
    json!({ "default_channel_ids": [], "prompts": [] })
}

fn probed(
    discord: &Discord,
    (delivery, recorded): (&Delivery, Recorded),
    answers: Vec<Response>,
) -> (Vec<Probe>, Vec<String>) {
    let mut scripted = vec![answer(200, &json!({ "id": BOT }))];
    scripted.extend(answers);
    let http = ScriptedHttp::new(scripted);
    let probes = pollster::block_on(discord.probe(&http, delivery, recorded));
    let asked = http.asked().into_iter().map(|(url, _)| url).collect();
    (probes, asked)
}

fn checked(
    discord: &Discord,
    needs: (&Delivery, Recorded),
    (channel, server, member): (Value, Value, Value),
) -> Vec<Probe> {
    let answers = vec![
        answer(200, &channel),
        answer(200, &server),
        answer(200, &member),
        answer(200, &onboarding()),
    ];
    let (mut probes, _) = probed(discord, needs, answers);
    assert_eq!(probes.first(), Some(&Probe::Taken));
    probes.remove(0);
    probes
}

fn lacks(name: &str, uses: &str) -> Probe {
    Probe::Route {
        key: Some("channel"),
        why: format!(
            "the bot lacks {name} in the channel, so herald cannot {uses}; give the bot's role {name} there"
        ),
    }
}

fn everything() -> Delivery {
    Delivery {
        thread: Some(Thread {
            name: "{{ tag }}".to_owned(),
            archive_after: Some(SignedDuration::from_hours(24)),
            lock_after: None,
        }),
        pin: Some(Pinning::Markers),
        mentions: Mentions {
            roles: vec![RoleId::new(NEWS), RoleId::new(QUIET), RoleId::new(OTHER)],
            subscribers: false,
        },
        edits: Edits::Release,
        ..Delivery::default()
    }
}

#[test]
fn doctor_names_each_permission_a_route_needs_that_the_bot_lacks_and_what_herald_cannot_do_without_it()
 {
    let probes = checked(
        &discord(false, Some(QUIET)),
        (&everything(), Recorded::Posts),
        (
            channel(0, &[overwrite(HERALD, ROLE, (VIEW, 0))]),
            server("1", &roles()),
            held(&[HERALD]),
        ),
    );
    assert_eq!(
        probes,
        [
            lacks("Send Messages in Threads", "post the further parts of a long announcement in its thread"),
            lacks("Create Public Threads", "open a thread from each announcement"),
            lacks("Manage Threads", "rename a thread when its release is edited or archive and lock the threads it opened"),
            lacks("Pin Messages", "pin each announcement and unpin the route's earlier one"),
            lacks("Read Message History", "find the route's earlier pin by its marker or read its posts back for the markers the store finds its records in"),
            Probe::Route {
                key: Some("mentions"),
                why: format!("the role Quiet ({QUIET}) is not mentionable and the bot lacks Mention @everyone, @here, and All Roles in the channel, so herald cannot ping it; make the role mentionable, or give the bot's role that permission there"),
            },
            Probe::Route {
                key: Some("mentions"),
                why: format!("the server has no role {OTHER}, so herald cannot ping it; mention a role of the channel's server"),
            },
            Probe::Route {
                key: Some("subscribe"),
                why: "the bot lacks Manage Roles in the server, so herald cannot give or take the opt-in role as members subscribe and unsubscribe; give the bot's role Manage Roles".to_owned(),
            },
            Probe::Route {
                key: Some("subscribe"),
                why: format!("the opt-in role Quiet ({QUIET}) is not below the bot's highest role, so herald cannot give or take it; move the bot's role above it in the server's roles"),
            },
        ]
    );
}

#[test]
fn a_route_that_only_posts_needs_only_to_see_and_post_in_the_channel() {
    let plain = (&Delivery::default(), Recorded::Kept);
    let granted = checked(
        &discord(false, None),
        plain,
        (
            channel(0, &[overwrite(HERALD, ROLE, (VIEW, 0))]),
            server("1", &roles()),
            held(&[HERALD]),
        ),
    );
    assert_eq!(granted, []);
    let unseen = checked(
        &discord(false, None),
        plain,
        (channel(0, &[]), server("1", &roles()), held(&[HERALD])),
    );
    assert_eq!(
        unseen,
        [
            lacks("View Channel", "see the channel"),
            lacks("Send Messages", "post each announcement"),
        ]
    );
}

#[test]
fn overwrites_lay_over_the_server_s_permissions_as_discord_lays_them() {
    let plain = (&Delivery::default(), Recorded::Kept);
    let probe = |overwrites: &[Value]| {
        checked(
            &discord(false, None),
            plain,
            (
                channel(0, overwrites),
                server("1", &roles()),
                held(&[HERALD, NEWS]),
            ),
        )
    };
    let allowed_over_denied = probe(&[
        overwrite(HERALD, ROLE, (VIEW, SEND)),
        overwrite(NEWS, ROLE, (SEND, 0)),
    ]);
    assert_eq!(allowed_over_denied, []);
    let own_denial = probe(&[
        overwrite(HERALD, ROLE, (VIEW, 0)),
        overwrite(BOT, MEMBER, (0, SEND)),
    ]);
    assert_eq!(
        own_denial,
        [lacks("Send Messages", "post each announcement")]
    );
    let own_grant = probe(&[overwrite(BOT, MEMBER, (VIEW, 0))]);
    assert_eq!(
        own_grant,
        [Probe::Public(format!(
            "the member {BOT} may view the channel and the route's viewers does not list them"
        ))]
    );
}

#[test]
fn a_role_is_pinged_where_the_bot_may_mention_every_role_and_send_there() {
    let mentioning = Delivery {
        mentions: Mentions {
            roles: vec![RoleId::new(QUIET)],
            subscribers: false,
        },
        ..Delivery::default()
    };
    let probe = |allow: u64, deny: u64| {
        checked(
            &discord(false, None),
            (&mentioning, Recorded::Kept),
            (
                channel(0, &[overwrite(HERALD, ROLE, (allow, deny))]),
                server("1", &roles()),
                held(&[HERALD]),
            ),
        )
    };
    assert_eq!(probe(VIEW | MENTION_EVERYONE, 0), []);
    let silenced = probe(VIEW | MENTION_EVERYONE, SEND);
    assert_eq!(silenced.len(), 2);
    assert_eq!(
        silenced.first(),
        Some(&lacks("Send Messages", "post each announcement"))
    );
}

#[test]
fn a_forum_needs_what_its_posts_need_and_reads_no_posts_back() {
    let pinned = Delivery {
        pin: Some(Pinning::Store),
        ..Delivery::default()
    };
    let probes = checked(
        &discord(true, None),
        (&pinned, Recorded::Posts),
        (
            channel(FORUM, &[overwrite(HERALD, ROLE, (VIEW, SEND))]),
            server("1", &roles()),
            held(&[HERALD]),
        ),
    );
    assert_eq!(
        probes,
        [
            lacks("Send Messages", "open a post for each announcement"),
            lacks(
                "Send Messages in Threads",
                "post the further parts of a long announcement in its thread"
            ),
            lacks("Manage Threads", "pin each announcement's post"),
        ]
    );
}

#[test]
fn the_server_s_owner_and_an_administrator_lack_nothing() {
    let needs = (&everything(), Recorded::Posts);
    let owned = checked(
        &discord(false, Some(NEWS)),
        needs,
        (channel(0, &[]), server(BOT, &roles()), held(&[])),
    );
    assert_eq!(
        owned,
        [Probe::Route {
            key: Some("mentions"),
            why: format!(
                "the server has no role {OTHER}, so herald cannot ping it; mention a role of the channel's server"
            ),
        }]
    );
    let mut ruling = roles();
    ruling.push(role(OTHER, "Admin", ADMINISTRATOR, (9, true)));
    let administered = checked(
        &discord(false, Some(NEWS)),
        needs,
        (channel(0, &[]), server("1", &ruling), held(&[OTHER])),
    );
    assert_eq!(
        administered,
        [Probe::Public(format!(
            "the role Admin ({OTHER}) may view the channel and the route's viewers does not list them"
        ))]
    );
}

#[test]
fn a_bot_that_manages_roles_below_its_own_gives_the_opt_in_role() {
    let mut managing = roles();
    managing.push(role(
        OTHER,
        "Manager",
        MANAGE_ROLES | READ_HISTORY,
        (3, false),
    ));
    let probes = checked(
        &discord(false, Some(NEWS)),
        (&Delivery::default(), Recorded::Kept),
        (
            channel(0, &[overwrite(HERALD, ROLE, (VIEW, 0))]),
            server("1", &managing),
            held(&[HERALD, OTHER]),
        ),
    );
    assert_eq!(probes, []);
    let missing = checked(
        &discord(false, Some(OTHER)),
        (&Delivery::default(), Recorded::Kept),
        (
            channel(0, &[overwrite(HERALD, ROLE, (VIEW, 0))]),
            server("1", &roles()),
            held(&[]),
        ),
    );
    assert_eq!(
        missing.last(),
        Some(&Probe::Route {
            key: Some("subscribe"),
            why: format!(
                "the server has no role {OTHER}, so no member can subscribe to it; name a role of the channel's server"
            ),
        })
    );
}

fn unchecked(why: &str) -> Probe {
    Probe::Said(format!(
        "herald does not check the bot's permissions in the channel: {why}"
    ))
}

#[test]
fn doctor_says_where_it_could_not_read_what_the_bot_may_do() {
    let plain = (&Delivery::default(), Recorded::Kept);
    let refused = probed(
        &discord(false, None),
        plain,
        vec![
            answer(200, &channel(0, &[])),
            answer(200, &server("1", &roles())),
            answer(403, &json!({ "code": 50001 })),
        ],
    )
    .0;
    assert_eq!(
        refused.get(1),
        Some(&unchecked(
            "Discord would not say which roles the bot holds, as it answered 403"
        ))
    );
    let unanswered = probed(
        &discord(false, None),
        plain,
        vec![
            answer(200, &channel(0, &[])),
            answer(200, &server("1", &roles())),
        ],
    )
    .0;
    assert!(matches!(
        unanswered.get(1),
        Some(Probe::Said(said)) if said.starts_with("herald does not check the bot's permissions in the channel: Discord could not be asked which roles the bot holds: ")
    ));
    let odd_member = checked(
        &discord(false, None),
        plain,
        (
            channel(0, &[]),
            server("1", &roles()),
            json!({ "roles": "x" }),
        ),
    );
    assert_eq!(
        odd_member,
        [unchecked(
            "Discord's answer did not say which roles the bot holds"
        )]
    );
    let odd_role = checked(
        &discord(false, None),
        plain,
        (
            channel(0, &[]),
            server("1", &[role(HERALD, "Herald", 0, (1, false))])
                .to_string()
                .replace("\"0\"", "\"x\"")
                .parse()
                .unwrap(),
            held(&[HERALD]),
        ),
    );
    assert_eq!(
        odd_role.first(),
        Some(&unchecked(
            "Discord did not say what one of the bot's roles may do"
        ))
    );
    let odd_overwrite = checked(
        &discord(false, None),
        plain,
        (
            channel(
                0,
                &[json!({ "id": HERALD, "type": ROLE, "allow": "x", "deny": "0" })],
            ),
            server("1", &roles()),
            held(&[HERALD]),
        ),
    );
    assert_eq!(
        odd_overwrite.first(),
        Some(&unchecked(
            "Discord did not say what the channel lets the bot do"
        ))
    );
}

#[test]
fn doctor_does_not_check_a_thread_nor_a_bot_discord_does_not_name() {
    let plain = (&Delivery::default(), Recorded::Kept);
    let (threaded, asked) = probed(
        &discord(false, None),
        plain,
        vec![answer(200, &channel(THREAD, &[]))],
    );
    let why = "the route's channel is a thread, whose audience herald does not read; route to its parent channel";
    assert_eq!(
        threaded,
        [
            Probe::Taken,
            unchecked(why),
            Probe::Said(format!(
                "herald withholds advisories sections from this channel: {why}"
            )),
        ]
    );
    assert_eq!(asked.len(), 2);
    let http = ScriptedHttp::new(vec![
        answer(200, &json!({ "name": "herald" })),
        answer(200, &channel(0, &[overwrite(HERALD, ROLE, (VIEW, 0))])),
        answer(200, &server("1", &roles())),
        answer(200, &onboarding()),
    ]);
    let unnamed = pollster::block_on(discord(false, None).probe(&http, plain.0, plain.1));
    assert_eq!(
        unnamed,
        [
            Probe::Taken,
            unchecked("Discord did not say who the bot is")
        ]
    );
    let http = ScriptedHttp::new(vec![
        answer(200, &json!({ "id": ".." })),
        answer(200, &channel(0, &[overwrite(HERALD, ROLE, (VIEW, 0))])),
        answer(200, &server("1", &roles())),
        answer(200, &onboarding()),
    ]);
    let dotted = pollster::block_on(discord(false, None).probe(&http, plain.0, plain.1));
    assert!(matches!(dotted.get(1), Some(Probe::Said(_))));
    assert_eq!(dotted.len(), 2);
}

#[test]
fn a_channel_discord_does_not_describe_is_not_checked() {
    let (probes, _) = probed(
        &discord(false, None),
        (&Delivery::default(), Recorded::Kept),
        vec![answer(200, &json!({ "type": "text" }))],
    );
    assert_eq!(
        probes,
        [
            Probe::Taken,
            Probe::Said("herald withholds advisories sections from this channel: Discord's answer did not say who may view the channel".to_owned()),
        ]
    );
}
