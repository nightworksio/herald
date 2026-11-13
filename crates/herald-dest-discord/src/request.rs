use herald_core::{
    Announcement, Button, Chore, Declined, Delivery, Entry, Phrase, Step, Wording, digest,
};
use herald_ports::{Credential, Method, Request, Secret, Text};
use serde_json::{Map, Value, json};

use crate::discord::{DISCORD, Discord};
use crate::interact::subscribe_button;
use crate::interactions::Subscribe;

/// The flag of a forum post pinned to the top of its forum.
pub(crate) const PINNED: u64 = 2;
/// A row of message components.
const ACTION_ROW: u8 = 1;
/// The most buttons a row holds.
const ROW: usize = 5;
/// The longest address a link button takes.
const BUTTON_URL: usize = 512;
/// A button component.
const BUTTON: u8 = 2;
/// The style of a button that opens a link.
const LINK: u8 = 5;
/// The style of a button that asks something of herald.
const SECONDARY: u8 = 2;
/// The longest deduplication key Discord takes, in a message's `nonce`.
const KEY_LENGTH: usize = 25;
pub(crate) const USER_AGENT: &str = concat!(
    "DiscordBot (https://github.com/nightworksio/herald, ",
    env!("CARGO_PKG_VERSION"),
    ")"
);

/// Why a webhook cannot take a step.
pub(crate) const fn webhook_cannot(what: &'static str) -> Declined {
    Declined::WebhookCannot {
        platform: DISCORD,
        what,
    }
}

/// Why a bot with no channel cannot take a step.
pub(crate) const NO_CHANNEL: Declined = Declined::NoChannel { platform: DISCORD };
/// Why Discord takes no delivery to an inbox.
const NO_INBOX: Declined = Declined::Cannot {
    platform: DISCORD,
    what: "deliver to an inbox",
};

/// A request to Discord, sent as the bot where `bot` holds its token.
pub(crate) fn bare(method: Method, url: Text, bot: Option<&Secret>) -> Request {
    let request = Request::new(method, url).header("User-Agent", Text::plain(USER_AGENT));
    match bot {
        Some(token) => request.header(
            "Authorization",
            Text::plain("Bot ").then_secret(token.clone()),
        ),
        None => request,
    }
}

/// A request to Discord carrying `body` as JSON, sent as the bot where
/// `bot` holds its token.
pub(crate) fn json(method: Method, url: Text, bot: Option<&Secret>, body: String) -> Request {
    bare(method, url, bot)
        .header("Content-Type", Text::plain("application/json"))
        .body(body)
}

fn message(
    discord: &Discord,
    announcement: &Announcement,
    delivery: &Delivery,
    deduplicated: bool,
    subscribe: Option<&Subscribe>,
) -> Value {
    let look = discord.look;
    let roles: Vec<&str> = delivery
        .mentions
        .roles
        .iter()
        .map(herald_core::RoleId::as_str)
        .collect();
    let content: Vec<String> = roles.iter().map(|role| format!("<@&{role}>")).collect();
    let mut embed = Map::new();
    embed.insert("title".to_owned(), json!(announcement.title.plain()));
    embed.insert(
        "description".to_owned(),
        json!(
            announcement
                .parts
                .first()
                .map(|part| part.body.as_str())
                .unwrap_or_default()
        ),
    );
    embed.insert(
        "color".to_owned(),
        json!(
            if announcement.prerelease {
                look.prerelease
            } else {
                look.release
            }
            .0
        ),
    );
    embed.insert(
        "timestamp".to_owned(),
        json!(announcement.occurred.to_string()),
    );
    embed.extend(
        announcement
            .link
            .iter()
            .map(|link| ("url".to_owned(), json!(announcement.marks.on(link)))),
    );
    let key = announcement.key.to_string();
    let mut body = Map::new();
    body.insert("content".to_owned(), json!(content.join(" ")));
    body.insert("embeds".to_owned(), json!([embed]));
    body.extend(
        buttons(&discord.wording, announcement, subscribe)
            .map(|components| ("components".to_owned(), components)),
    );
    body.insert(
        "allowed_mentions".to_owned(),
        json!({ "parse": [], "roles": roles }),
    );
    body.extend(deduplicated.then(|| {
        (
            "nonce".to_owned(),
            json!(key.chars().take(KEY_LENGTH).collect::<String>()),
        )
    }));
    body.extend(deduplicated.then(|| ("enforce_nonce".to_owned(), json!(true))));
    Value::Object(body)
}

fn buttons(
    wording: &Wording,
    announcement: &Announcement,
    subscribe: Option<&Subscribe>,
) -> Option<Value> {
    let subscribing = subscribe
        .filter(|subscribe| subscribe.button)
        .and_then(|_| subscribe_button(&announcement.project))
        .map(|id| json!({ "type": BUTTON, "style": SECONDARY, "label": wording.text(Phrase::SubscribeButton), "custom_id": id }));
    let buttons: Vec<Value> = announcement
        .buttons
        .iter()
        .filter(|button| fits(button))
        .map(|button| json!({ "type": BUTTON, "style": LINK, "label": button.label(wording), "url": button.url }))
        .chain(subscribing)
        .collect();
    (!buttons.is_empty()).then(|| {
        buttons
            .chunks(ROW)
            .map(|row| json!({ "type": ACTION_ROW, "components": row }))
            .collect()
    })
}

/// Whether Discord takes a link button to where `button` leads.
fn fits(button: &Button) -> bool {
    button.url.len() <= BUTTON_URL
}

fn thread_name(announcement: &Announcement) -> String {
    announcement
        .thread
        .clone()
        .unwrap_or_else(|| announcement.title.plain().to_owned())
}

/// The overflow chunk `index` of `announcement`. A bot's chunk carries a
/// nonce of its own, so Discord takes it once however often it is sent; a
/// webhook takes none.
fn chunk(announcement: &Announcement, index: u32, deduplicated: bool) -> Value {
    let part = announcement.part(index);
    let mut body = Map::new();
    body.insert(
        "content".to_owned(),
        json!(part.map(|part| part.body.as_str()).unwrap_or_default()),
    );
    body.insert("allowed_mentions".to_owned(), json!({ "parse": [] }));
    if deduplicated {
        body.insert("nonce".to_owned(), json!(chunk_nonce(announcement, index)));
        body.insert("enforce_nonce".to_owned(), json!(true));
    }
    Value::Object(body)
}

/// The nonce of a bot's overflow chunk `index`: a digest of the
/// announcement's key and the index, as long as Discord takes.
fn chunk_nonce(announcement: &Announcement, index: u32) -> String {
    digest(&format!("{}/{index}", announcement.key))
        .chars()
        .take(KEY_LENGTH)
        .collect()
}

pub(crate) fn posted(entry: &Entry) -> Result<&str, Declined> {
    entry
        .message(&Step::Post)
        .ok_or(Declined::Missing("the announcement's message"))
}

fn thread(entry: &Entry) -> Option<&str> {
    [Step::OpenThread, Step::Post]
        .iter()
        .filter_map(|step| entry.facts(step))
        .find_map(|facts| facts.thread.as_deref())
        .filter(|thread| !thread.is_empty())
}

pub(crate) fn step(
    discord: &Discord,
    step: &Step,
    announcement: &Announcement,
    delivery: &Delivery,
    entry: &Entry,
) -> Result<Request, Declined> {
    match &discord.credential {
        Credential::Webhook(url) => webhook(discord, url, step, announcement, delivery, entry),
        Credential::Bot(_) => bot(discord, step, announcement, delivery, entry),
    }
}

fn webhook(
    discord: &Discord,
    url: &Secret,
    step: &Step,
    announcement: &Announcement,
    delivery: &Delivery,
    entry: &Entry,
) -> Result<Request, Declined> {
    let forum = discord.route.forum;
    match step {
        Step::Post => {
            let mut body = message(discord, announcement, delivery, false, None);
            if let (true, Value::Object(fields)) = (forum, &mut body) {
                fields.insert("thread_name".to_owned(), json!(thread_name(announcement)));
            }
            Ok(json(
                Method::Post,
                Text::secret(url.clone()).then("?wait=true&with_components=true"),
                None,
                body.to_string(),
            ))
        }
        Step::Chunk(index) => {
            let target = match thread(entry) {
                Some(thread) => {
                    Text::secret(url.clone()).then(format!("?wait=true&thread_id={thread}"))
                }
                None => Text::secret(url.clone()).then("?wait=true"),
            };
            Ok(json(
                Method::Post,
                target,
                None,
                chunk(announcement, *index, false).to_string(),
            ))
        }
        Step::Crosspost => Err(webhook_cannot("crosspost")),
        Step::OpenThread => Err(webhook_cannot("open a thread from a message")),
        Step::Pin => Err(webhook_cannot("pin")),
        Step::Unpin => Err(webhook_cannot("unpin")),
        Step::Deliver(_) | Step::Deliveries => Err(NO_INBOX),
    }
}

fn bot(
    discord: &Discord,
    step: &Step,
    announcement: &Announcement,
    delivery: &Delivery,
    entry: &Entry,
) -> Result<Request, Declined> {
    let channel = channel(discord)?;
    let api = &discord.api;
    let bot = discord.credential.bot();
    match step {
        Step::Post if discord.route.forum => {
            let name = thread_name(announcement);
            let body = json!({
                "name": name,
                "auto_archive_duration": discord.look.archive_after.minutes(),
                "message": message(discord, announcement, delivery, false, discord.route.subscribe.as_ref()),
            });
            Ok(json(
                Method::Post,
                Text::plain(format!("{api}/channels/{channel}/threads")),
                bot,
                body.to_string(),
            ))
        }
        Step::Post => {
            let url = Text::plain(format!("{api}/channels/{channel}/messages"));
            let body = message(
                discord,
                announcement,
                delivery,
                true,
                discord.route.subscribe.as_ref(),
            );
            Ok(json(Method::Post, url, bot, body.to_string()))
        }
        Step::Crosspost => {
            let message = posted(entry)?;
            let url = Text::plain(format!(
                "{api}/channels/{channel}/messages/{message}/crosspost"
            ));
            Ok(bare(Method::Post, url, bot))
        }
        Step::OpenThread => {
            let message = posted(entry)?;
            let name = thread_name(announcement);
            let body = json!({ "name": name, "auto_archive_duration": discord.look.archive_after.minutes() });
            let url = Text::plain(format!(
                "{api}/channels/{channel}/messages/{message}/threads"
            ));
            Ok(json(Method::Post, url, bot, body.to_string()))
        }
        Step::Chunk(index) => {
            let target = thread(entry).unwrap_or(channel);
            let url = Text::plain(format!("{api}/channels/{target}/messages"));
            Ok(json(
                Method::Post,
                url,
                bot,
                chunk(announcement, *index, true).to_string(),
            ))
        }
        Step::Pin if discord.route.forum => {
            let post = posted(entry)?;
            let url = Text::plain(format!("{api}/channels/{post}"));
            Ok(json(
                Method::Patch,
                url,
                bot,
                json!({ "flags": PINNED }).to_string(),
            ))
        }
        Step::Pin => {
            let message = posted(entry)?;
            let url = Text::plain(format!("{api}/channels/{channel}/messages/pins/{message}"));
            Ok(bare(Method::Put, url, bot))
        }
        Step::Unpin => {
            posted(entry)?;
            let earlier = announcement
                .replaces
                .as_deref()
                .ok_or(Declined::Missing("the route's earlier pin"))?;
            Ok(unpin(discord, channel, earlier))
        }
        Step::Deliver(_) | Step::Deliveries => Err(NO_INBOX),
    }
}

/// The route's channel, which a bot needs.
pub(crate) fn channel(discord: &Discord) -> Result<&str, Declined> {
    discord
        .route
        .channel
        .as_deref()
        .filter(|channel| !channel.is_empty())
        .ok_or(NO_CHANNEL)
}

pub(crate) fn get(discord: &Discord, path: &str) -> Request {
    let url = format!("{}{path}", discord.api);
    bare(Method::Get, Text::plain(url), discord.credential.bot())
}

pub(crate) fn unpin(discord: &Discord, channel: &str, message: &str) -> Request {
    let api = &discord.api;
    let bot = discord.credential.bot();
    if discord.route.forum {
        let url = Text::plain(format!("{api}/channels/{message}"));
        return json(Method::Patch, url, bot, json!({ "flags": 0 }).to_string());
    }
    let url = format!("{api}/channels/{channel}/messages/pins/{message}");
    bare(Method::Delete, Text::plain(url), bot)
}

pub(crate) fn chore(
    discord: &Discord,
    housekeeping: Chore,
    thread: &str,
) -> Result<Vec<Request>, Declined> {
    let Some(bot) = discord.credential.bot() else {
        return Err(webhook_cannot("archive or lock a thread"));
    };
    let bodies = match housekeeping {
        Chore::Archive => vec![json!({ "archived": true })],
        Chore::Lock => vec![
            json!({ "archived": false, "locked": true }),
            json!({ "archived": true }),
        ],
    };
    let url = format!("{}/channels/{thread}", discord.api);
    Ok(bodies
        .iter()
        .map(|body| {
            json(
                Method::Patch,
                Text::plain(url.clone()),
                Some(bot),
                body.to_string(),
            )
        })
        .collect())
}
