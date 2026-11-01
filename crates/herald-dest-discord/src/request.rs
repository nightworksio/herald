use std::fmt;

use herald_core::{Announcement, Chore, Delivery, Entry, Marker, Step};
use herald_ports::{Method, Request, Text};
use serde_json::{Map, Value, json};

use crate::discord::{Credential, Discord};

/// The colour of a release's embed: the gold of herald's trumpet.
const RELEASE: u32 = 0x00E3_9A26;
/// The colour of a pre-release's embed: the violet of herald's banner.
const PRERELEASE: u32 = 0x008F_88C7;
/// A week, the longest Discord keeps a quiet thread open.
const ARCHIVE_MINUTES: u32 = 10_080;
/// The longest nonce Discord takes.
const NONCE: usize = 25;
const USER_AGENT: &str = concat!(
    "DiscordBot (https://github.com/nightworksio/herald, ",
    env!("CARGO_PKG_VERSION"),
    ")"
);

/// Why a step cannot be asked of this destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A webhook cannot take the step.
    WebhookCannot(&'static str),
    /// A bot has no channel to post to.
    NoChannel,
    /// The step needs an earlier step's answer, which is not recorded.
    Missing(&'static str),
    /// Discord delivers to no inbox.
    NoInbox,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WebhookCannot(what) => {
                write!(f, "a Discord webhook cannot {what}; use a bot token")
            }
            Self::NoChannel => f.write_str("the route names no Discord channel; set its channel"),
            Self::Missing(what) => write!(
                f,
                "{what} is not recorded, so this step cannot be taken yet"
            ),
            Self::NoInbox => f.write_str("Discord delivers to no inbox"),
        }
    }
}

fn json(method: Method, url: Text, credential: &Credential, body: &Value) -> Request {
    let request = Request::new(method, url)
        .header("Content-Type", Text::plain("application/json"))
        .header("User-Agent", Text::plain(USER_AGENT))
        .body(body.to_string());
    authorised(request, credential)
}

pub(crate) fn bare(method: Method, url: Text, credential: &Credential) -> Request {
    authorised(
        Request::new(method, url).header("User-Agent", Text::plain(USER_AGENT)),
        credential,
    )
}

fn authorised(request: Request, credential: &Credential) -> Request {
    match credential {
        Credential::Bot(token) => request.header(
            "Authorization",
            Text::plain("Bot ").then_secret(token.clone()),
        ),
        Credential::Webhook(_) => request,
    }
}

pub(crate) fn fragment(marker: &Marker) -> String {
    format!("#herald-{marker}")
}

fn marked(link: &str, announcement: &Announcement) -> String {
    let page = link.split('#').next().unwrap_or(link);
    format!("{page}{}", fragment(&announcement.marker))
}

fn message(announcement: &Announcement, delivery: &Delivery, nonce: bool) -> Value {
    let roles: Vec<&str> = delivery
        .mentions
        .roles
        .iter()
        .map(herald_core::RoleId::as_str)
        .collect();
    let content: Vec<String> = roles.iter().map(|role| format!("<@&{role}>")).collect();
    let mut embed = Map::new();
    embed.insert("title".to_owned(), json!(announcement.title));
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
        json!(if announcement.prerelease {
            PRERELEASE
        } else {
            RELEASE
        }),
    );
    embed.insert(
        "timestamp".to_owned(),
        json!(announcement.occurred.to_string()),
    );
    embed.extend(
        announcement
            .link
            .iter()
            .map(|link| ("url".to_owned(), json!(marked(link, announcement)))),
    );
    let key = announcement.key.to_string();
    let mut body = Map::new();
    body.insert("content".to_owned(), json!(content.join(" ")));
    body.insert("embeds".to_owned(), json!([embed]));
    body.insert(
        "allowed_mentions".to_owned(),
        json!({ "parse": [], "roles": roles }),
    );
    body.extend(nonce.then(|| {
        (
            "nonce".to_owned(),
            json!(key.chars().take(NONCE).collect::<String>()),
        )
    }));
    body.extend(nonce.then(|| ("enforce_nonce".to_owned(), json!(true))));
    Value::Object(body)
}

fn thread_name(announcement: &Announcement) -> String {
    announcement
        .thread
        .clone()
        .unwrap_or_else(|| announcement.title.clone())
}

fn chunk(announcement: &Announcement, index: u32) -> Value {
    let part = usize::try_from(index)
        .ok()
        .and_then(|index| announcement.parts.get(index));
    json!({
        "content": part.map(|part| part.body.as_str()).unwrap_or_default(),
        "allowed_mentions": { "parse": [] },
    })
}

fn landed<'a>(
    entry: &'a Entry,
    step: &Step,
    what: &'static str,
) -> Result<&'a herald_core::Facts, Refusal> {
    entry.facts(step).ok_or(Refusal::Missing(what))
}

fn posted(entry: &Entry) -> Result<&str, Refusal> {
    landed(entry, &Step::Post, "the announcement's message")?
        .message
        .as_deref()
        .ok_or(Refusal::Missing("the announcement's message"))
}

fn thread(entry: &Entry) -> Option<&str> {
    [Step::OpenThread, Step::Post]
        .iter()
        .filter_map(|step| entry.facts(step))
        .find_map(|facts| facts.thread.as_deref())
}

pub(crate) fn step(
    discord: &Discord,
    step: &Step,
    announcement: &Announcement,
    delivery: &Delivery,
    entry: &Entry,
) -> Result<Request, Refusal> {
    match &discord.credential {
        Credential::Webhook(url) => webhook(
            url,
            step,
            announcement,
            delivery,
            entry,
            discord.route.forum,
        ),
        Credential::Bot(_) => bot(discord, step, announcement, delivery, entry),
    }
}

fn webhook(
    url: &herald_ports::Secret,
    step: &Step,
    announcement: &Announcement,
    delivery: &Delivery,
    entry: &Entry,
    forum: bool,
) -> Result<Request, Refusal> {
    let credential = Credential::Webhook(url.clone());
    match step {
        Step::Post => {
            let mut body = message(announcement, delivery, false);
            if let (true, Value::Object(fields)) = (forum, &mut body) {
                fields.insert("thread_name".to_owned(), json!(thread_name(announcement)));
            }
            Ok(json(
                Method::Post,
                Text::secret(url.clone()).then("?wait=true"),
                &credential,
                &body,
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
                &credential,
                &chunk(announcement, *index),
            ))
        }
        Step::Crosspost => Err(Refusal::WebhookCannot("crosspost")),
        Step::OpenThread => Err(Refusal::WebhookCannot("open a thread from a message")),
        Step::Pin => Err(Refusal::WebhookCannot("pin")),
        Step::Deliver(_) => Err(Refusal::NoInbox),
    }
}

fn bot(
    discord: &Discord,
    step: &Step,
    announcement: &Announcement,
    delivery: &Delivery,
    entry: &Entry,
) -> Result<Request, Refusal> {
    let channel = discord.route.channel.as_deref().ok_or(Refusal::NoChannel)?;
    let api = &discord.api;
    let credential = &discord.credential;
    match step {
        Step::Post if discord.route.forum => {
            let name = thread_name(announcement);
            let body = json!({
                "name": name,
                "auto_archive_duration": ARCHIVE_MINUTES,
                "message": message(announcement, delivery, false),
            });
            Ok(json(
                Method::Post,
                Text::plain(format!("{api}/channels/{channel}/threads")),
                credential,
                &body,
            ))
        }
        Step::Post => {
            let url = Text::plain(format!("{api}/channels/{channel}/messages"));
            Ok(json(
                Method::Post,
                url,
                credential,
                &message(announcement, delivery, true),
            ))
        }
        Step::Crosspost => {
            let message = posted(entry)?;
            let url = Text::plain(format!(
                "{api}/channels/{channel}/messages/{message}/crosspost"
            ));
            Ok(bare(Method::Post, url, credential))
        }
        Step::OpenThread => {
            let message = posted(entry)?;
            let name = thread_name(announcement);
            let body = json!({ "name": name, "auto_archive_duration": ARCHIVE_MINUTES });
            let url = Text::plain(format!(
                "{api}/channels/{channel}/messages/{message}/threads"
            ));
            Ok(json(Method::Post, url, credential, &body))
        }
        Step::Chunk(index) => {
            let target = thread(entry).unwrap_or(channel);
            let url = Text::plain(format!("{api}/channels/{target}/messages"));
            Ok(json(
                Method::Post,
                url,
                credential,
                &chunk(announcement, *index),
            ))
        }
        Step::Pin => {
            let message = posted(entry)?;
            let url = Text::plain(format!("{api}/channels/{channel}/messages/pins/{message}"));
            Ok(bare(Method::Put, url, credential))
        }
        Step::Deliver(_) => Err(Refusal::NoInbox),
    }
}

pub(crate) fn chore(
    discord: &Discord,
    housekeeping: Chore,
    thread: &str,
) -> Result<Vec<Request>, Refusal> {
    let Credential::Bot(_) = &discord.credential else {
        return Err(Refusal::WebhookCannot("archive or lock a thread"));
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
                &discord.credential,
                body,
            )
        })
        .collect())
}
