//! The permissions the bot holds in its route's channel, resolved as
//! Discord resolves them, against those the route's delivery needs
//! (ADR-0002, ADR-0004).

use std::collections::BTreeMap;

use herald_core::{Delivery, Edits, Pinning};
use herald_kinds::{Probe, Recorded};
use herald_ports::Http;
use serde::Deserialize;

use crate::audience::{Channel, Guild, MEMBER, Role, bits, laid};
use crate::discord::Discord;
use crate::request::{get, path};

/// The permission to view a channel.
pub(crate) const VIEW_CHANNEL: u64 = 1 << 10;
/// The permission that grants every other.
pub(crate) const ADMINISTRATOR: u64 = 1 << 3;
/// The permission to post in a channel and to open a forum's posts.
const SEND_MESSAGES: u64 = 1 << 11;
/// The permission to post in a thread.
const SEND_MESSAGES_IN_THREADS: u64 = 1 << 38;
/// The permission to open a thread from a message.
const CREATE_PUBLIC_THREADS: u64 = 1 << 35;
/// The permission to archive, lock and rename a thread, and to pin a
/// forum's post.
const MANAGE_THREADS: u64 = 1 << 34;
/// The permission to pin and unpin a message.
const PIN_MESSAGES: u64 = 1 << 51;
/// The permission to read a channel's earlier messages and pins.
const READ_MESSAGE_HISTORY: u64 = 1 << 16;
/// The permission to ping a role that is not mentionable.
const MENTION_EVERYONE: u64 = 1 << 17;
/// The permission to give and take a server's roles.
const MANAGE_ROLES: u64 = 1 << 28;

/// The channel permissions a route may need, as Discord names them, in
/// the order doctor says them.
const NAMED: [(u64, &str); 7] = [
    (VIEW_CHANNEL, "View Channel"),
    (SEND_MESSAGES, "Send Messages"),
    (SEND_MESSAGES_IN_THREADS, "Send Messages in Threads"),
    (CREATE_PUBLIC_THREADS, "Create Public Threads"),
    (MANAGE_THREADS, "Manage Threads"),
    (PIN_MESSAGES, "Pin Messages"),
    (READ_MESSAGE_HISTORY, "Read Message History"),
];

/// The bot as a member of the channel's server.
#[derive(Deserialize)]
struct Member {
    roles: Vec<String>,
}

/// The route's channel and the server it is in, by its id, as read.
#[derive(Clone, Copy)]
pub(crate) struct Placed<'a> {
    pub(crate) guild: &'a str,
    pub(crate) server: &'a Guild,
    pub(crate) channel: &'a Channel,
}

/// What the bot `bot` lacks in the channel `placed` names, of what a route
/// delivering as `delivery`, its announcements `recorded` so, needs, asked
/// through `http`.
pub(crate) async fn lacking<H: Http>(
    discord: &Discord,
    http: &H,
    (bot, placed): (&str, Placed<'_>),
    (delivery, recorded): (&Delivery, Recorded),
) -> Vec<Probe> {
    let unchecked = |why: String| {
        vec![Probe::Said(format!(
            "herald does not check the bot's permissions in the channel: {why}"
        ))]
    };
    let member = match member(discord, http, placed.guild, bot).await {
        Ok(member) => member,
        Err(why) => return unchecked(why),
    };
    let owner = placed.server.owner_id.as_deref() == Some(bot);
    let Some(base) = base(placed, &member, owner) else {
        return unchecked("Discord did not say what one of the bot's roles may do".to_owned());
    };
    let Some(granted) = channelled(base, placed, &member, bot) else {
        return unchecked("Discord did not say what the channel lets the bot do".to_owned());
    };
    let needed = needed(discord, delivery, recorded);
    let mut probes: Vec<Probe> = NAMED
        .iter()
        .filter(|(permission, _)| granted & permission == 0)
        .filter_map(|(permission, name)| {
            let uses = needed.get(permission)?;
            Some(Probe::Route {
                key: Some("channel"),
                why: format!(
                    "the bot lacks {name} in the channel, so herald cannot {}; give the bot's role {name} there",
                    uses.join(" or ")
                ),
            })
        })
        .collect();
    probes.extend(mentioned(placed.server, delivery, granted));
    if let Some(subscribe) = &discord.route.subscribe {
        probes.extend(subscribing(
            placed.server,
            &member,
            base,
            owner,
            &subscribe.role,
        ));
    }
    probes
}

/// The bot's membership of `guild`, read through `http`, or why it is not.
async fn member<H: Http>(
    discord: &Discord,
    http: &H,
    guild: &str,
    bot: &str,
) -> Result<Member, String> {
    let path = path(&["guilds", guild, "members", bot]).map_err(|declined| declined.to_string())?;
    let answer = http.send(get(discord, &path)).await.map_err(|error| {
        format!("Discord could not be asked which roles the bot holds: {error}")
    })?;
    if !answer.succeeded() {
        return Err(format!(
            "Discord would not say which roles the bot holds, as it answered {}",
            answer.status
        ));
    }
    serde_json::from_slice(&answer.body)
        .map_err(|_| "Discord's answer did not say which roles the bot holds".to_owned())
}

/// The roles of `server` that `member` holds.
fn held<'a>(server: &'a Guild, member: &'a Member) -> impl Iterator<Item = &'a Role> {
    server
        .roles
        .iter()
        .filter(|role| member.roles.contains(&role.id))
}

/// The bot's permissions across the server, before any channel's
/// overwrites: every permission for its owner, else `@everyone`'s with
/// each of its roles', every permission where one is Administrator.
fn base(placed: Placed<'_>, member: &Member, owner: bool) -> Option<u64> {
    if owner {
        return Some(u64::MAX);
    }
    let everyone = placed
        .server
        .roles
        .iter()
        .find(|role| role.id == placed.guild)
        .and_then(|role| bits(&role.permissions))?;
    let mut permissions = everyone;
    for role in held(placed.server, member) {
        permissions |= bits(&role.permissions)?;
    }
    Some(if permissions & ADMINISTRATOR == 0 {
        permissions
    } else {
        u64::MAX
    })
}

/// `base` with the channel's overwrites laid over it as Discord lays them:
/// `@everyone`'s, then those of the bot's roles together, then the bot's
/// own. Without View Channel nothing else is left, and without Send
/// Messages no role is pinged.
fn channelled(base: u64, placed: Placed<'_>, member: &Member, bot: &str) -> Option<u64> {
    if base & ADMINISTRATOR != 0 {
        return Some(u64::MAX);
    }
    let overwrites = &placed.channel.permission_overwrites;
    let of = |id: &str, kind: fn(u8) -> bool| {
        overwrites
            .iter()
            .find(|overwrite| kind(overwrite.kind) && overwrite.id == id)
    };
    let role = |kind: u8| kind != MEMBER;
    let own = |kind: u8| kind == MEMBER;
    let mut permissions = laid(base, of(placed.guild, role))?;
    let (mut allow, mut deny) = (0, 0);
    for id in &member.roles {
        if let Some(overwrite) = of(id, role) {
            allow |= bits(&overwrite.allow)?;
            deny |= bits(&overwrite.deny)?;
        }
    }
    permissions = (permissions & !deny) | allow;
    permissions = laid(permissions, of(bot, own))?;
    if permissions & VIEW_CHANNEL == 0 {
        return Some(0);
    }
    if permissions & SEND_MESSAGES == 0 {
        permissions &= !MENTION_EVERYONE;
    }
    Some(permissions)
}

/// Each channel permission a route of `discord` delivering as `delivery`,
/// its announcements `recorded` so, needs, with what herald does with it.
fn needed(
    discord: &Discord,
    delivery: &Delivery,
    recorded: Recorded,
) -> BTreeMap<u64, Vec<&'static str>> {
    let forum = discord.route.forum;
    let thread = delivery.thread.as_ref();
    let mut needed: BTreeMap<u64, Vec<&'static str>> = BTreeMap::new();
    let mut need = |permission: u64, uses: &'static str| {
        needed.entry(permission).or_default().push(uses);
    };
    need(VIEW_CHANNEL, "see the channel");
    if forum {
        need(SEND_MESSAGES, "open a post for each announcement");
    } else {
        need(SEND_MESSAGES, "post each announcement");
    }
    if forum || thread.is_some() {
        need(
            SEND_MESSAGES_IN_THREADS,
            "post the further parts of a long announcement in its thread",
        );
    }
    if let (false, Some(thread)) = (forum, thread) {
        need(
            CREATE_PUBLIC_THREADS,
            "open a thread from each announcement",
        );
        if delivery.edits != Edits::None {
            need(MANAGE_THREADS, "rename a thread when its release is edited");
        }
        if thread.archive_after.is_some() || thread.lock_after.is_some() {
            need(MANAGE_THREADS, "archive and lock the threads it opened");
        }
    }
    match (&delivery.pin, forum) {
        (None, _) => {}
        (Some(_), true) => need(MANAGE_THREADS, "pin each announcement's post"),
        (Some(_), false) => need(
            PIN_MESSAGES,
            "pin each announcement and unpin the route's earlier one",
        ),
    }
    if delivery.pin == Some(Pinning::Markers) {
        need(
            READ_MESSAGE_HISTORY,
            "find the route's earlier pin by its marker",
        );
    }
    if recorded == Recorded::Posts && !forum {
        need(
            READ_MESSAGE_HISTORY,
            "read its posts back for the markers the store finds its records in",
        );
    }
    needed
}

/// What the route's mentions lack where the bot, granted `granted` in the
/// channel, pings a role of `server` that is not mentionable, or one the
/// server does not have.
fn mentioned(server: &Guild, delivery: &Delivery, granted: u64) -> Vec<Probe> {
    delivery
        .mentions
        .roles
        .iter()
        .filter_map(|mentioned| {
            let id = mentioned.as_str();
            let why = match server.roles.iter().find(|role| role.id == id) {
                None => format!("the server has no role {id}, so herald cannot ping it; mention a role of the channel's server"),
                Some(role) if role.mentionable || granted & MENTION_EVERYONE != 0 => return None,
                Some(role) => format!(
                    "the role {} ({id}) is not mentionable and the bot lacks Mention @everyone, @here, and All Roles in the channel, so herald cannot ping it; make the role mentionable, or give the bot's role that permission there",
                    role.name
                ),
            };
            Some(Probe::Route {
                key: Some("mentions"),
                why,
            })
        })
        .collect()
}

/// What the bot, holding `base` across the server and the roles `member`
/// lists, lacks to give and take the opt-in role `role`: Manage Roles, and
/// a role of its own above it. The server's owner lacks neither.
fn subscribing(server: &Guild, member: &Member, base: u64, owner: bool, role: &str) -> Vec<Probe> {
    let mut probes = Vec::new();
    let lacks = |why: String| Probe::Route {
        key: Some("subscribe"),
        why,
    };
    if base & MANAGE_ROLES == 0 {
        probes.push(lacks("the bot lacks Manage Roles in the server, so herald cannot give or take the opt-in role as members subscribe and unsubscribe; give the bot's role Manage Roles".to_owned()));
    }
    let Some(opt_in) = server.roles.iter().find(|found| found.id == role) else {
        probes.push(lacks(format!("the server has no role {role}, so no member can subscribe to it; name a role of the channel's server")));
        return probes;
    };
    let highest = held(server, member).map(|held| held.position).max();
    if !owner && highest.is_none_or(|highest| highest <= opt_in.position) {
        probes.push(lacks(format!(
            "the opt-in role {} ({role}) is not below the bot's highest role, so herald cannot give or take it; move the bot's role above it in the server's roles",
            opt_in.name
        )));
    }
    probes
}
