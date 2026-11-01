use herald_core::{Announcement, Entry, Facts, Marker, Sent};
use herald_ports::{Http, HttpError};
use serde_json::Value;

use crate::discord::{Credential, Discord};
use crate::markers::{author, marks};
use crate::read::trouble;
use crate::request::{PINNED, Refusal, get, posted, unpin};

/// The most pins Discord lists in one page.
const PINS: u8 = 50;
/// What Discord answers to unpinning a message no longer there.
const GONE: u16 = 404;

/// An answer read, a reason to stop with, or no answer at all.
type Read<T> = Result<Result<T, Sent>, HttpError>;

async fn read<H: Http>(discord: &Discord, http: &H, path: &str) -> Read<Value> {
    let response = http.send(get(discord, path)).await?;
    let body = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    Ok(trouble(&response, &body).map_or(Ok(body), Err))
}

fn field<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name).and_then(Value::as_str)
}

fn routed(message: &Value, bot: &str, route: &Marker) -> bool {
    author(message) == Some(bot)
        && marks(message)
            .iter()
            .any(|(marked, _)| marked == route.as_str())
}

async fn bot<H: Http>(discord: &Discord, http: &H) -> Read<String> {
    let user = match read(discord, http, "/users/@me").await? {
        Ok(user) => user,
        Err(sent) => return Ok(Err(sent)),
    };
    Ok(field(&user, "id")
        .map(ToOwned::to_owned)
        .ok_or_else(|| Sent::Refused("Discord did not say which user the bot is".to_owned())))
}

async fn pinned_messages<H: Http>(
    discord: &Discord,
    http: &H,
    channel: &str,
    bot: &str,
    route: &Marker,
) -> Read<Vec<String>> {
    let path = format!("/channels/{channel}/messages/pins?limit={PINS}");
    let pins = match read(discord, http, &path).await? {
        Ok(pins) => pins,
        Err(sent) => return Ok(Err(sent)),
    };
    Ok(Ok(pins
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|pin| pin.get("message"))
        .filter(|message| routed(message, bot, route))
        .filter_map(|message| field(message, "id"))
        .map(ToOwned::to_owned)
        .collect()))
}

async fn pinned_posts<H: Http>(
    discord: &Discord,
    http: &H,
    forum: &str,
    bot: &str,
    route: &Marker,
) -> Read<Vec<String>> {
    let channel = match read(discord, http, &format!("/channels/{forum}")).await? {
        Ok(channel) => channel,
        Err(sent) => return Ok(Err(sent)),
    };
    let guild = field(&channel, "guild_id").unwrap_or_default();
    let active = match read(discord, http, &format!("/guilds/{guild}/threads/active")).await? {
        Ok(active) => active,
        Err(sent) => return Ok(Err(sent)),
    };
    let candidates: Vec<String> = active
        .get("threads")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|thread| field(thread, "parent_id") == Some(forum))
        .filter(|thread| field(thread, "owner_id") == Some(bot))
        .filter(|thread| {
            thread
                .get("flags")
                .and_then(Value::as_u64)
                .is_some_and(|flags| flags & PINNED != 0)
        })
        .filter_map(|thread| field(thread, "id"))
        .map(ToOwned::to_owned)
        .collect();
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
) -> Read<Vec<String>> {
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
    if let Credential::Webhook(_) = discord.credential {
        return Ok(Sent::Refused(Refusal::WebhookCannot("unpin").to_string()));
    }
    let Some(channel) = discord.route.channel.as_deref() else {
        return Ok(Sent::Refused(Refusal::NoChannel.to_string()));
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
        let body = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
        if let Some(sent) = trouble(&response, &body) {
            return Ok(sent);
        }
    }
    Ok(Sent::Landed(Facts::default()))
}
