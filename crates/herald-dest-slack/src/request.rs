use std::fmt;

use herald_core::{Announcement, Dialect, Entry, Step, safe_url};
use herald_ports::{Method, Request, Text, USER_AGENT, unreserved};
use serde_json::{Value, json};

use crate::interact::subscribe_block;
use crate::interactions::Subscribe;
use crate::slack::{Credential, Slack};
use crate::web_api::{CHAT_POST_MESSAGE, PINS_ADD, PINS_LIST, WebMethod};

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
pub(crate) fn get(slack: &Slack, method: WebMethod, query: &str) -> Request {
    let name = method.name;
    let team = slack
        .team
        .as_deref()
        .map(|team| format!("team_id={}", unreserved(team)));
    let query = [Some(query), team.as_deref()]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("&");
    let url = if query.is_empty() {
        format!("{}/{name}", slack.api)
    } else {
        format!("{}/{name}?{query}", slack.api)
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
pub(crate) fn call(slack: &Slack, method: WebMethod, body: &Value) -> Request {
    let mut body = body.clone();
    if let (Some(team), Value::Object(fields)) = (&slack.team, &mut body) {
        fields.insert("team_id".to_owned(), Value::String(team.clone()));
    }
    posted_json(
        slack,
        Text::plain(format!("{}/{}", slack.api, method.name)),
        &body,
    )
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

fn announcement_body(announcement: &Announcement, subscribe: Option<&Subscribe>) -> Value {
    let first = announcement
        .parts
        .first()
        .map(|part| part.body.as_str())
        .filter(|text| !text.is_empty());
    let link = announcement.link.as_deref().map(link);
    let mentions = mentioning(&announcement.mentioned);
    let mut blocks = vec![json!({
        "type": "header",
        "text": { "type": "plain_text", "text": announcement.title.plain(), "emoji": true },
    })];
    let mut fallback = vec![announcement.title.shown(Dialect::Slack).body];
    for text in first
        .map(ToOwned::to_owned)
        .into_iter()
        .chain(link)
        .chain(mentions)
    {
        blocks.push(section(&text));
        fallback.push(text);
    }
    if subscribe.is_some_and(|subscribe| subscribe.button) {
        blocks.push(subscribe_block(&announcement.project));
    }
    json!({
        "text": fallback.join("\n\n"),
        "blocks": blocks,
        "unfurl_links": false,
        "unfurl_media": false,
    })
}

/// `users` as one `mrkdwn` text that mentions each, where any is a user id.
fn mentioning(users: &[String]) -> Option<String> {
    let mentions: Vec<String> = users
        .iter()
        .filter(|user| user_id(user))
        .map(|user| format!("<@{user}>"))
        .collect();
    (!mentions.is_empty()).then(|| mentions.join(" "))
}

fn chunk_body(announcement: &Announcement, index: u32) -> Value {
    let index = usize::try_from(index).unwrap_or(usize::MAX);
    let text = announcement.mentioned_in(index).map_or_else(
        || {
            announcement
                .parts
                .get(index)
                .map(|part| part.body.clone())
                .unwrap_or_default()
        },
        |users| mentioning(users).unwrap_or_default(),
    );
    let text = text.as_str();
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
        (_, Step::Deliver(_) | Step::Deliveries) => Err(Refusal::Cannot("deliver to an inbox")),
        (Credential::Webhook(url), Step::Post) => Ok(posted_json(
            slack,
            Text::secret(url.clone()),
            &announcement_body(announcement, slack.route.subscribe.as_ref()),
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
                announcement_body(announcement, slack.route.subscribe.as_ref()),
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
            Ok(call(slack, CHAT_POST_MESSAGE, &body))
        }
        (Credential::Bot(_), Step::Chunk(index)) => {
            let channel = channel(slack)?;
            let root = posted(entry, &Step::Post, "the announcement's message")?;
            let body = with(
                chunk_body(announcement, *index),
                &[("channel", json!(channel)), ("thread_ts", json!(root))],
            );
            Ok(call(slack, CHAT_POST_MESSAGE, &body))
        }
        (Credential::Bot(_), Step::Pin) => {
            let channel = channel(slack)?;
            let post = posted(entry, &Step::Post, "the announcement's message")?;
            Ok(pin(slack, PINS_ADD, channel, post))
        }
        (Credential::Bot(_), Step::Unpin) => Ok(pins(slack, channel(slack)?)),
    }
}

/// The call pinning or unpinning, as `method` says, the message `ts`.
pub(crate) fn pin(slack: &Slack, method: WebMethod, channel: &str, ts: &str) -> Request {
    call(
        slack,
        method,
        &json!({ "channel": channel, "timestamp": ts }),
    )
}

/// The read of the channel's pins.
pub(crate) fn pins(slack: &Slack, channel: &str) -> Request {
    get(
        slack,
        PINS_LIST,
        &format!("channel={}", unreserved(channel)),
    )
}
