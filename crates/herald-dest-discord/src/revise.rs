//! A post brought to a new revision: each part edited, each chunk no
//! longer needed deleted, and the thread opened from it renamed (ADR-0034).

use herald_core::{Announcement, Declined, Delivery, Entry, NO_PART};
use herald_ports::{Method, Request, Secret, SecretText};
use serde_json::json;

use crate::discord::Discord;
use crate::request::{bare, chunk, endpoint, json, message, path, recorded, thread, thread_name};

/// Why a thread cannot be renamed: it is not recorded.
const NO_THREAD: Declined = Declined::Missing("the thread opened from the announcement");

/// The message of `part` in the record `entry` keeps.
fn part(entry: &Entry, part: u32) -> Result<&str, Declined> {
    entry.part(part).ok_or(NO_PART).and_then(recorded)
}

/// What `part` of `announcement` reads, edited: the post's message, or an
/// overflow chunk's.
fn body(discord: &Discord, part: u32, announcement: &Announcement, delivery: &Delivery) -> String {
    match part {
        0 => message(
            discord,
            announcement,
            delivery,
            false,
            discord.route.subscribe.as_ref(),
        ),
        chunk_part => chunk(announcement, chunk_part, None),
    }
    .to_string()
}

/// The webhook's address of the message `message`, in the thread `entry`
/// records where it records one.
fn webhook_message(
    url: &Secret,
    message: &str,
    entry: &Entry,
    components: bool,
) -> Result<SecretText, Declined> {
    let query: Vec<String> = thread(entry)?
        .map(|thread| format!("thread_id={}", herald_core::unreserved(thread)))
        .into_iter()
        .chain(components.then(|| "with_components=true".to_owned()))
        .collect();
    let query = if query.is_empty() {
        String::new()
    } else {
        format!("?{}", query.join("&"))
    };
    Ok(SecretText::secret(url.clone()).then(format!("{}{query}", path(&["messages", message])?)))
}

/// The webhook's edit of `part` of `announcement`.
pub(crate) fn webhook_edit(
    discord: &Discord,
    url: &Secret,
    edited: u32,
    announcement: &Announcement,
    delivery: &Delivery,
    entry: &Entry,
) -> Result<Request, Declined> {
    let target = webhook_message(url, part(entry, edited)?, entry, edited == 0)?;
    Ok(json(
        Method::Patch,
        target,
        None,
        body(discord, edited, announcement, delivery),
    ))
}

/// The webhook's deletion of the overflow chunk `deleted`.
pub(crate) fn webhook_delete(
    url: &Secret,
    deleted: u32,
    entry: &Entry,
) -> Result<Request, Declined> {
    let target = webhook_message(url, part(entry, deleted)?, entry, false)?;
    Ok(bare(Method::Delete, target, None))
}

/// The channel the bot's message of `part` is in: a forum post's thread, or
/// the thread its chunks follow in, else the route's channel.
fn bot_channel<'a>(
    discord: &Discord,
    channel: &'a str,
    entry: &'a Entry,
    part: u32,
) -> Result<&'a str, Declined> {
    match (part, discord.route.forum) {
        (0, false) => Ok(channel),
        _ => Ok(thread(entry)?.unwrap_or(channel)),
    }
}

/// The bot's edit of `part` of `announcement`.
pub(crate) fn bot_edit(
    discord: &Discord,
    channel: &str,
    edited: u32,
    announcement: &Announcement,
    delivery: &Delivery,
    entry: &Entry,
) -> Result<Request, Declined> {
    let message = part(entry, edited)?;
    let target = bot_channel(discord, channel, entry, edited)?;
    Ok(json(
        Method::Patch,
        endpoint(&discord.api, &["channels", target, "messages", message])?,
        discord.credential.bot(),
        body(discord, edited, announcement, delivery),
    ))
}

/// The bot's deletion of the overflow chunk `deleted`.
pub(crate) fn bot_delete(
    discord: &Discord,
    channel: &str,
    deleted: u32,
    entry: &Entry,
) -> Result<Request, Declined> {
    let message = part(entry, deleted)?;
    let target = bot_channel(discord, channel, entry, deleted)?;
    Ok(bare(
        Method::Delete,
        endpoint(&discord.api, &["channels", target, "messages", message])?,
        discord.credential.bot(),
    ))
}

/// The bot's rename of the thread opened from the post to `announcement`'s
/// name.
pub(crate) fn bot_rename(
    discord: &Discord,
    announcement: &Announcement,
    entry: &Entry,
) -> Result<Request, Declined> {
    let opened = thread(entry)?.ok_or(NO_THREAD)?;
    Ok(json(
        Method::Patch,
        endpoint(&discord.api, &["channels", opened])?,
        discord.credential.bot(),
        json!({ "name": thread_name(announcement) }).to_string(),
    ))
}
