use std::fmt;

use herald_core::{Announcement, Dialect, Entry, Step, literal, safe_url};
use herald_ports::{Method, Request, Text, USER_AGENT};
use serde_json::{Value, json};

use crate::slack::{Credential, Slack};

/// The type of the metadata event herald's posts carry their markers in
/// (ADR-0011).
pub(crate) const EVENT: &str = "herald_announcement";
/// The field of the metadata's payload carrying the route's marker.
pub(crate) const ROUTE: &str = "route";
/// The field of the metadata's payload carrying the announcement's marker.
pub(crate) const MARKER: &str = "marker";

/// Why a step cannot be asked of Slack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Slack has no such thing to do.
    Cannot(&'static str),
    /// An incoming webhook cannot do it; a bot token can.
    WebhookCannot(&'static str),
    /// A bot needs the route's channel, and the route names none.
    NoChannel,
    /// The step needs an earlier step's answer, which is not recorded.
    Missing(&'static str),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cannot(what) => write!(f, "Slack cannot {what}"),
            Self::WebhookCannot(what) => write!(
                f,
                "a Slack webhook cannot {what}; reach Slack with a bot token to"
            ),
            Self::NoChannel => {
                f.write_str("a Slack bot posts to a channel, and the route names none")
            }
            Self::Missing(what) => write!(
                f,
                "{what} is not recorded, so this step cannot be taken yet"
            ),
        }
    }
}

fn authorised(request: Request, slack: &Slack) -> Request {
    let request = request.header("User-Agent", Text::plain(USER_AGENT));
    match &slack.credential {
        Credential::Bot(token) => request.header(
            "Authorization",
            Text::plain("Bearer ").then_secret(token.clone()),
        ),
        Credential::Webhook(_) => request,
    }
}

/// A call of the Web API method `method`, with `query` already encoded.
pub(crate) fn get(slack: &Slack, method: &str, query: &str) -> Request {
    let url = if query.is_empty() {
        format!("{}/{method}", slack.api)
    } else {
        format!("{}/{method}?{query}", slack.api)
    };
    authorised(Request::new(Method::Get, Text::plain(url)), slack)
}

fn posted_json(slack: &Slack, url: Text, body: &Value) -> Request {
    authorised(Request::new(Method::Post, url), slack)
        .header(
            "Content-Type",
            Text::plain("application/json; charset=utf-8"),
        )
        .body(body.to_string())
}

/// A call of the Web API method `method` with the JSON `body`.
pub(crate) fn call(slack: &Slack, method: &str, body: &Value) -> Request {
    posted_json(slack, Text::plain(format!("{}/{method}", slack.api)), body)
}

/// `text` as one part of a URL's query: every byte but the unreserved ones
/// percent-encoded.
pub(crate) fn query(text: &str) -> String {
    use std::fmt::Write as _;
    text.bytes().fold(String::new(), |mut encoded, byte| {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
        encoded
    })
}

/// Whether `user` is written as a Slack user id, so it can be mentioned.
fn user_id(user: &str) -> bool {
    user.strip_prefix(['U', 'W']).is_some_and(|rest| {
        rest.len() >= 2
            && rest
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    })
}

/// `url` as a link `mrkdwn` reads whole: made safe, with `&` escaped.
fn link(url: &str) -> String {
    format!("<{}>", safe_url(url).replace('&', "&amp;"))
}

fn section(text: &str) -> Value {
    json!({ "type": "section", "text": { "type": "mrkdwn", "text": text } })
}

fn announcement_body(announcement: &Announcement) -> Value {
    let first = announcement
        .parts
        .first()
        .map(|part| part.body.as_str())
        .filter(|text| !text.is_empty());
    let link = announcement.link.as_deref().map(link);
    let mentions: Vec<String> = announcement
        .mentioned
        .iter()
        .filter(|user| user_id(user))
        .map(|user| format!("<@{user}>"))
        .collect();
    let mut blocks = vec![json!({
        "type": "header",
        "text": { "type": "plain_text", "text": announcement.title, "emoji": true },
    })];
    let mut fallback = vec![literal(&announcement.title, Dialect::Slack).body];
    for text in first
        .map(ToOwned::to_owned)
        .into_iter()
        .chain(link)
        .chain((!mentions.is_empty()).then(|| mentions.join(" ")))
    {
        blocks.push(section(&text));
        fallback.push(text);
    }
    json!({
        "text": fallback.join("\n\n"),
        "blocks": blocks,
        "unfurl_links": false,
        "unfurl_media": false,
    })
}

fn chunk_body(announcement: &Announcement, index: u32) -> Value {
    let text = usize::try_from(index)
        .ok()
        .and_then(|index| announcement.parts.get(index))
        .map(|part| part.body.as_str())
        .unwrap_or_default();
    json!({
        "text": text,
        "blocks": [section(text)],
        "unfurl_links": false,
        "unfurl_media": false,
    })
}

fn with(mut body: Value, fields: &[(&str, Value)]) -> Value {
    body.as_object_mut().into_iter().for_each(|object| {
        object.extend(
            fields
                .iter()
                .map(|(name, value)| ((*name).to_owned(), value.clone())),
        );
    });
    body
}

/// The timestamp of the message `step` made, as `entry` recorded it.
pub(crate) fn posted<'a>(
    entry: &'a Entry,
    step: &Step,
    what: &'static str,
) -> Result<&'a str, Refusal> {
    entry
        .facts(step)
        .and_then(|facts| facts.message.as_deref())
        .ok_or(Refusal::Missing(what))
}

/// The route's channel, which a bot needs.
pub(crate) fn channel(slack: &Slack) -> Result<&str, Refusal> {
    slack.route.channel.as_deref().ok_or(Refusal::NoChannel)
}

pub(crate) fn step(
    slack: &Slack,
    step: &Step,
    announcement: &Announcement,
    entry: &Entry,
) -> Result<Request, Refusal> {
    match (&slack.credential, step) {
        (_, Step::Crosspost) => Err(Refusal::Cannot("crosspost")),
        (_, Step::OpenThread) => Err(Refusal::Cannot("open a thread; its replies are the thread")),
        (_, Step::Deliver(_)) => Err(Refusal::Cannot("deliver to an inbox")),
        (Credential::Webhook(url), Step::Post) => Ok(posted_json(
            slack,
            Text::secret(url.clone()),
            &announcement_body(announcement),
        )),
        (Credential::Webhook(url), Step::Chunk(index)) => Ok(posted_json(
            slack,
            Text::secret(url.clone()),
            &chunk_body(announcement, *index),
        )),
        (Credential::Webhook(_), Step::Pin) => Err(Refusal::WebhookCannot("pin")),
        (Credential::Webhook(_), Step::Unpin) => Err(Refusal::WebhookCannot("unpin")),
        (Credential::Bot(_), Step::Post) => {
            let marks = &announcement.marks;
            let body = with(
                announcement_body(announcement),
                &[
                    ("channel", json!(channel(slack)?)),
                    (
                        "metadata",
                        json!({
                            "event_type": EVENT,
                            "event_payload": {
                                ROUTE: marks.route.as_str(),
                                MARKER: marks.announcement.as_str(),
                            },
                        }),
                    ),
                ],
            );
            Ok(call(slack, "chat.postMessage", &body))
        }
        (Credential::Bot(_), Step::Chunk(index)) => {
            let channel = channel(slack)?;
            let root = posted(entry, &Step::Post, "the announcement's message")?;
            let body = with(
                chunk_body(announcement, *index),
                &[("channel", json!(channel)), ("thread_ts", json!(root))],
            );
            Ok(call(slack, "chat.postMessage", &body))
        }
        (Credential::Bot(_), Step::Pin) => {
            let channel = channel(slack)?;
            let post = posted(entry, &Step::Post, "the announcement's message")?;
            Ok(pin(slack, "pins.add", channel, post))
        }
        (Credential::Bot(_), Step::Unpin) => Ok(pins(slack, channel(slack)?)),
    }
}

/// The call pinning or unpinning, as `method` says, the message `ts`.
pub(crate) fn pin(slack: &Slack, method: &str, channel: &str, ts: &str) -> Request {
    call(
        slack,
        method,
        &json!({ "channel": channel, "timestamp": ts }),
    )
}

/// The read of the channel's pins.
pub(crate) fn pins(slack: &Slack, channel: &str) -> Request {
    get(slack, "pins.list", &format!("channel={}", query(channel)))
}
