//! Who may view a Discord route's channel, asked right before a run's first
//! advisories section is sent there: a channel is private only where its
//! server's `@everyone` may not view it, every role that may is one the
//! route's `viewers` lists, no member is let in by an overwrite of their
//! own the route does not list, and the server's onboarding hands neither
//! the channel nor such a role to whoever joins (ADR-0067).

use std::collections::BTreeSet;

use herald_kinds::Audience;
use herald_ports::Http;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::discord::Discord;
use crate::interactions::snowflake;
use crate::request::{get, path};

/// What herald asks before a run's first advisory send, as doctor names
/// it.
pub const ASKS: &str = "Discord for the channel's permission overwrites, its server's roles and its server's onboarding, through the bot token, which must see the channel";

/// Why a route reached by a webhook is not checked, and how to make it
/// checkable.
pub const UNCHECKED: &str = "a Discord webhook cannot read who may view its channel; give the destination a bot token that sees the channel, so herald checks it";

/// The requests the check takes: the channel, its server, then the
/// server's onboarding.
pub(crate) const REQUESTS: usize = 3;

/// The permission to view a channel.
const VIEW_CHANNEL: u64 = 1 << 10;
/// The permission that grants every other.
const ADMINISTRATOR: u64 = 1 << 3;
/// The channel types of a thread, whose audience is its parent's.
const THREADS: [u8; 3] = [10, 11, 12];
/// The overwrite type of one member.
const MEMBER: u8 = 1;

/// A channel as Discord answers for it.
#[derive(Deserialize)]
pub(crate) struct Channel {
    #[serde(rename = "type")]
    kind: u8,
    guild_id: Option<String>,
    #[serde(default)]
    permission_overwrites: Vec<Overwrite>,
}

#[derive(Deserialize)]
struct Overwrite {
    id: String,
    #[serde(rename = "type")]
    kind: u8,
    allow: String,
    deny: String,
}

#[derive(Deserialize)]
struct Guild {
    roles: Vec<Role>,
}

#[derive(Deserialize)]
struct Role {
    id: String,
    name: String,
    permissions: String,
}

#[derive(Deserialize)]
struct Onboarding {
    default_channel_ids: Vec<String>,
    prompts: Vec<Prompt>,
}

#[derive(Deserialize)]
struct Prompt {
    options: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    #[serde(default)]
    channel_ids: Vec<String>,
    #[serde(default)]
    role_ids: Vec<String>,
}

/// What Discord answers at `path` through `http`, read as a `T`; or why
/// it could not be.
async fn read<T: DeserializeOwned, H: Http>(
    discord: &Discord,
    http: &H,
    path: &[&str],
) -> Result<T, String> {
    let path = self::path(path).map_err(|declined| declined.to_string())?;
    let answer = http
        .send(get(discord, &path))
        .await
        .map_err(|error| format!("Discord could not be asked who may view the channel: {error}"))?;
    if !answer.succeeded() {
        return Err(format!(
            "Discord would not say who may view the channel, as it answered {}; give the bot View Channel there, and Manage Server for its onboarding",
            answer.status
        ));
    }
    serde_json::from_slice(&answer.body)
        .map_err(|_| "Discord's answer did not say who may view the channel".to_owned())
}

/// A permission bit field as Discord writes one.
fn bits(text: &str) -> Option<u64> {
    text.parse().ok()
}

/// The permissions `base` leaves once the overwrite `overwrite` is laid
/// over them; none where it is not written as Discord writes one.
fn laid(base: u64, overwrite: Option<&Overwrite>) -> Option<u64> {
    overwrite.map_or(Some(base), |overwrite| {
        Some((base & !bits(&overwrite.deny)?) | bits(&overwrite.allow)?)
    })
}

/// The roles of `server` other than `@everyone`, whose id is `guild`, that
/// may view a channel with `overwrites`, as Discord resolves permissions:
/// the role's own with `@everyone`'s, Administrator granting all, then the
/// channel's overwrite of `@everyone`, then its overwrite of the role. A
/// synced channel holds a copy of its category's overwrites, so the
/// channel's own are the ones Discord applies. `Err` where `@everyone` may
/// view it, or why it is unread.
fn viewing<'a>(
    server: &'a Guild,
    guild: &str,
    overwrites: &[Overwrite],
) -> Result<Vec<&'a Role>, Audience> {
    let unread = |what: &str| Audience::Unread(format!("Discord did not say what {what} may do"));
    let everyone = server
        .roles
        .iter()
        .find(|role| role.id == guild)
        .and_then(|role| bits(&role.permissions))
        .ok_or_else(|| unread("the server's @everyone"))?;
    let overwrite = |id: &str| {
        overwrites
            .iter()
            .find(|overwrite| overwrite.kind != MEMBER && overwrite.id == id)
    };
    let everyone_laid = |base: u64| {
        laid(base, overwrite(guild)).ok_or_else(|| unread("the channel lets @everyone"))
    };
    if everyone & ADMINISTRATOR != 0 || everyone_laid(everyone)? & VIEW_CHANNEL != 0 {
        return Err(Audience::Public(
            "everyone in its server may view the channel, as its @everyone role and the channel's permission overwrites allow View Channel".to_owned(),
        ));
    }
    let mut seeing = Vec::new();
    for role in server.roles.iter().filter(|role| role.id != guild) {
        let own = bits(&role.permissions).ok_or_else(|| unread(&role.name))?;
        let base = everyone | own;
        let viewed = base & ADMINISTRATOR != 0
            || laid(everyone_laid(base)?, overwrite(&role.id)).ok_or_else(|| unread(&role.name))?
                & VIEW_CHANNEL
                != 0;
        if viewed {
            seeing.push(role);
        }
    }
    Ok(seeing)
}

/// Who may view the channel of `discord`'s route, asked through `http`.
pub(crate) async fn audience<H: Http>(discord: &Discord, http: &H) -> Audience {
    if discord.credential.bot().is_none() {
        return Audience::Unread(UNCHECKED.to_owned());
    }
    let Some(channel) = discord
        .route
        .channel
        .as_deref()
        .filter(|channel| snowflake(channel))
    else {
        return Audience::Unread("the route names no channel id".to_owned());
    };
    let found = read(discord, http, &["channels", channel]).await;
    seen(discord, http, (channel, found)).await
}

/// Who may view `channel`, as Discord's answer for it, `found`, and the
/// further answers asked through `http` say: what the probe, which has
/// read the channel already, asks no second time.
pub(crate) async fn seen<H: Http>(
    discord: &Discord,
    http: &H,
    (channel, found): (&str, Result<Channel, String>),
) -> Audience {
    let found = match found {
        Ok(found) => found,
        Err(why) => return Audience::Unread(why),
    };
    if THREADS.contains(&found.kind) {
        return Audience::Unread(
            "the route's channel is a thread, whose audience herald does not read; route to its parent channel".to_owned(),
        );
    }
    let Some(guild) = found.guild_id.filter(|guild| snowflake(guild)) else {
        return Audience::Unread("Discord did not say which server the channel is in".to_owned());
    };
    let server: Guild = match read(discord, http, &["guilds", &guild]).await {
        Ok(server) => server,
        Err(why) => return Audience::Unread(why),
    };
    let seeing = match viewing(&server, &guild, &found.permission_overwrites) {
        Ok(seeing) => seeing,
        Err(audience) => return audience,
    };
    let listed: BTreeSet<&str> = discord.route.viewers.iter().map(String::as_str).collect();
    let mut extra: Vec<String> = seeing
        .iter()
        .filter(|role| !listed.contains(role.id.as_str()))
        .map(|role| format!("the role {} ({})", role.name, role.id))
        .collect();
    extra.extend(
        found
            .permission_overwrites
            .iter()
            .filter(|overwrite| overwrite.kind == MEMBER && !listed.contains(overwrite.id.as_str()))
            .filter(|overwrite| {
                bits(&overwrite.allow).is_none_or(|allow| allow & VIEW_CHANNEL != 0)
            })
            .map(|overwrite| format!("the member {}", overwrite.id)),
    );
    if !extra.is_empty() {
        return Audience::Public(format!(
            "{} may view the channel and the route's viewers does not list them",
            extra.join(", ")
        ));
    }
    let onboarding: Onboarding = match read(discord, http, &["guilds", &guild, "onboarding"]).await
    {
        Ok(onboarding) => onboarding,
        Err(why) => return Audience::Unread(why),
    };
    onboarded(&onboarding, channel, &seeing).unwrap_or(Audience::Private)
}

/// Why the server's `onboarding` lets whoever joins view `channel`: it
/// shows them the channel, or hands them one of the roles `seeing` it.
fn onboarded(onboarding: &Onboarding, channel: &str, seeing: &[&Role]) -> Option<Audience> {
    let choices = onboarding.prompts.iter().flat_map(|prompt| &prompt.options);
    let shown = onboarding
        .default_channel_ids
        .iter()
        .any(|id| id == channel)
        || choices
            .clone()
            .any(|choice| choice.channel_ids.iter().any(|id| id == channel));
    if shown {
        return Some(Audience::Public(
            "the server's onboarding shows the channel to whoever joins".to_owned(),
        ));
    }
    let handed = seeing.iter().find(|role| {
        choices
            .clone()
            .any(|choice| choice.role_ids.contains(&role.id))
    })?;
    Some(Audience::Public(format!(
        "the server's onboarding lets whoever joins take the role {} ({}), which may view the channel",
        handed.name, handed.id
    )))
}

#[cfg(test)]
mod tests;
