use herald_core::status;
use herald_core::{Announcement, Entry, Facts, Marker, Sent, unreserved};
use herald_ports::{Http, HttpError};
use serde_json::Value;

use crate::discord::Discord;
use crate::markers::{author, marks};
use crate::read::{failure, id, json};
use crate::request::{PINNED, channel, get, posted, unpin, webhook_cannot};

/// What is said where Discord names no user for the bot's token.
pub(crate) const UNNAMED: &str = "Discord did not say which user the bot is";

/// The most pins Discord lists in one page.
const PINS: u8 = 50;
/// What Discord answers to unpinning a message no longer there.
const GONE: u16 = status::NOT_FOUND;

/// An answer read, a reason to stop with, or no answer at all.
type ApiAnswer<T> = Result<Result<T, Sent>, HttpError>;

async fn read<H: Http>(discord: &Discord, http: &H, path: &str) -> ApiAnswer<Value> {
    let response = http.send(get(discord, path)).await?;
    Ok(failure(&response, discord.waits).map_or_else(|| Ok(json(&response)), Err))
}

fn routed(message: &Value, bot: &str, route: &Marker) -> bool {
    author(message) == Some(bot)
        && marks(message)
            .iter()
            .any(|(marked, _)| marked == route.as_str())
}

async fn bot<H: Http>(discord: &Discord, http: &H) -> ApiAnswer<String> {
    if let Some(bot) = discord.bot.get() {
        return Ok(Ok(bot.clone()));
    }
    let user = match read(discord, http, "/users/@me").await? {
        Ok(user) => user,
        Err(sent) => return Ok(Err(sent)),
    };
    Ok(id(&user, "id")
        .map(|bot| discord.bot.learn(bot.to_owned()).clone())
        .ok_or_else(|| Sent::Refused(UNNAMED.to_owned())))
}

async fn pinned_messages<H: Http>(
    discord: &Discord,
    http: &H,
    channel: &str,
    bot: &str,
    route: &Marker,
) -> ApiAnswer<Vec<String>> {
    let mut routed_pins = Vec::new();
    let mut before = None;
    for _ in 0..discord.read_back.get() {
        let path = before.as_deref().map_or_else(
            || format!("/channels/{channel}/messages/pins?limit={PINS}"),
            |at| {
                format!(
                    "/channels/{channel}/messages/pins?limit={PINS}&before={}",
                    unreserved(at)
                )
            },
        );
        let pins = match read(discord, http, &path).await? {
            Ok(pins) => pins,
            Err(sent) => return Ok(Err(sent)),
        };
        let items = pins
            .get("items")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        routed_pins.extend(
            items
                .iter()
                .filter_map(|pin| pin.get("message"))
                .filter(|message| routed(message, bot, route))
                .filter_map(|message| id(message, "id"))
                .map(ToOwned::to_owned),
        );
        before = items
            .last()
            .and_then(|pin| id(pin, "pinned_at"))
            .map(ToOwned::to_owned);
        if pins.get("has_more").and_then(Value::as_bool) != Some(true) || before.is_none() {
            break;
        }
    }
    Ok(Ok(routed_pins))
}

async fn pinned_posts<H: Http>(
    discord: &Discord,
    http: &H,
    forum: &str,
    bot: &str,
    route: &Marker,
) -> ApiAnswer<Vec<String>> {
    let channel = match read(discord, http, &format!("/channels/{forum}")).await? {
        Ok(channel) => channel,
        Err(sent) => return Ok(Err(sent)),
    };
    let Some(guild) = id(&channel, "guild_id") else {
        return Ok(Err(Sent::Refused(format!(
            "Discord did not say which server the forum {forum} is in"
        ))));
    };
    let active = match read(discord, http, &format!("/guilds/{guild}/threads/active")).await? {
        Ok(active) => active,
        Err(sent) => return Ok(Err(sent)),
    };
    let candidates: Vec<String> = active
        .get("threads")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|thread| id(thread, "parent_id") == Some(forum))
        .filter(|thread| id(thread, "owner_id") == Some(bot))
        .filter(|thread| {
            thread
                .get("flags")
                .and_then(Value::as_u64)
                .is_some_and(|flags| flags & PINNED != 0)
        })
        .filter_map(|thread| id(thread, "id"))
        .map(ToOwned::to_owned)
        .collect();
    if let Some(over) = discord.pin_lookups.over(candidates.len()) {
        return Ok(Err(Sent::Refused(over)));
    }
    let mut posts = Vec::new();
    for post in candidates {
        let path = format!("/channels/{post}/messages/{post}");
        match read(discord, http, &path).await? {
            Ok(message) if routed(&message, bot, route) => posts.push(post),
            Ok(_) => {}
            Err(sent) => return Ok(Err(sent)),
        }
    }
    Ok(Ok(posts))
}

async fn earlier<H: Http>(
    discord: &Discord,
    http: &H,
    channel: &str,
    route: &Marker,
) -> ApiAnswer<Vec<String>> {
    let bot = match bot(discord, http).await? {
        Ok(bot) => bot,
        Err(sent) => return Ok(Err(sent)),
    };
    if discord.route.forum {
        pinned_posts(discord, http, channel, &bot, route).await
    } else {
        pinned_messages(discord, http, channel, &bot, route).await
    }
}

/// Unpins the route's earlier pins: the one the store remembers, or else
/// each of the bot's own pins carrying the route's marker. The
/// announcement's own pin stays.
pub(crate) async fn unpinned<H: Http>(
    discord: &Discord,
    http: &H,
    announcement: &Announcement,
    entry: &Entry,
) -> Result<Sent, HttpError> {
    if discord.credential.bot().is_none() {
        return Ok(Sent::Refused(webhook_cannot("unpin").to_string()));
    }
    let channel = match channel(discord) {
        Ok(channel) => channel,
        Err(declined) => return Ok(Sent::Refused(declined.to_string())),
    };
    let current = match posted(entry) {
        Ok(current) => current.to_owned(),
        Err(refusal) => return Ok(Sent::Refused(refusal.to_string())),
    };
    let stale = match &announcement.replaces {
        Some(remembered) => vec![remembered.clone()],
        None => match earlier(discord, http, channel, &announcement.marks.route).await? {
            Ok(stale) => stale,
            Err(sent) => return Ok(sent),
        },
    };
    for message in stale.iter().filter(|message| **message != current) {
        let response = http.send(unpin(discord, channel, message)).await?;
        if response.status == GONE {
            continue;
        }
        if let Some(sent) = failure(&response, discord.waits) {
            return Ok(sent);
        }
    }
    Ok(Sent::Landed(Facts::default()))
}
