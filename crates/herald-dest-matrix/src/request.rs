use std::fmt;
use std::fmt::Write as _;

use herald_core::{Announcement, Dialect, Entry, Sent, Step, literal, safe_url};
use herald_ports::{Method, Request, Response, Text, USER_AGENT};
use serde_json::{Map, Value, json};

use crate::matrix::Matrix;
use crate::read::{json, trouble};

/// The field of a message's content carrying its route's marker (ADR-0011).
pub(crate) const ROUTE: &str = "io.nightworks.herald.route";
/// The field of a message's content carrying its announcement's marker
/// (ADR-0011).
pub(crate) const MARKER: &str = "io.nightworks.herald.marker";
/// The format of a message's sanitised HTML.
const HTML: &str = "org.matrix.custom.html";
/// Where a mention links, as Matrix clients read it.
const PILL: &str = "https://matrix.to/#/";
/// The state event holding a room's pinned events.
const PINNED_EVENTS: &str = "m.room.pinned_events";
/// The answer of a homeserver for state a room does not hold.
pub(crate) const NOT_FOUND: u16 = 404;

/// Why a step cannot be asked of Matrix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Matrix has no such thing to do.
    Cannot(&'static str),
    /// The step needs an earlier step's answer, which is not recorded.
    Missing(&'static str),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cannot(what) => write!(f, "Matrix cannot {what}"),
            Self::Missing(what) => write!(
                f,
                "{what} is not recorded, so this step cannot be taken yet"
            ),
        }
    }
}

/// `text` as one segment of a URL's path: every byte but the unreserved
/// ones percent-encoded.
pub(crate) fn segment(text: &str) -> String {
    text.bytes().fold(String::new(), |mut encoded, byte| {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
        encoded
    })
}

fn authorised(request: Request, matrix: &Matrix) -> Request {
    request
        .header("User-Agent", Text::plain(USER_AGENT))
        .header(
            "Authorization",
            Text::plain("Bearer ").then_secret(matrix.token.clone()),
        )
}

pub(crate) fn get(matrix: &Matrix, path: &str) -> Request {
    let url = format!("{}/_matrix/client/v3{path}", matrix.homeserver);
    authorised(Request::new(Method::Get, Text::plain(url)), matrix)
}

fn put(matrix: &Matrix, path: &str, body: &Value) -> Request {
    let url = format!("{}/_matrix/client/v3{path}", matrix.homeserver);
    authorised(Request::new(Method::Put, Text::plain(url)), matrix)
        .header("Content-Type", Text::plain("application/json"))
        .body(body.to_string())
}

fn room(matrix: &Matrix) -> String {
    format!("/rooms/{}", segment(&matrix.route.room))
}

/// Whether `user` is written as a Matrix user id, so it can be mentioned.
fn user_id(user: &str) -> bool {
    user.strip_prefix('@')
        .and_then(|rest| rest.split_once(':'))
        .is_some_and(|(local, server)| {
            !local.is_empty()
                && local.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._=-/+".contains(&byte)
                })
                && !server.is_empty()
                && server
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b".-:[]".contains(&byte))
        })
}

fn announcement_content(announcement: &Announcement) -> Value {
    let first = announcement.parts.first();
    let title = literal(&announcement.title, Dialect::Matrix);
    let link = announcement
        .link
        .as_deref()
        .map(|link| announcement.marks.on(link));
    let mentioned: Vec<&str> = announcement
        .mentioned
        .iter()
        .map(String::as_str)
        .filter(|user| user_id(user))
        .collect();
    let mut body = vec![title.body.clone()];
    let mut html = vec![format!(
        "<h4>{}</h4>",
        title.html.as_deref().unwrap_or_default().trim_end()
    )];
    if let Some(text) = first.filter(|text| !text.body.is_empty()) {
        body.push(text.body.clone());
        html.push(text.html.clone().unwrap_or_default());
    }
    if let Some(link) = &link {
        body.push(link.clone());
        let href = safe_url(link);
        html.push(format!("<p><a href=\"{href}\">{href}</a></p>"));
    }
    if !mentioned.is_empty() {
        body.push(mentioned.join(" "));
        let pills: Vec<String> = mentioned
            .iter()
            .map(|user| format!("<a href=\"{PILL}{user}\">{user}</a>"))
            .collect();
        html.push(format!("<p>{}</p>", pills.join(" ")));
    }
    let mut content = Map::new();
    content.insert("msgtype".to_owned(), json!("m.text"));
    content.insert("body".to_owned(), json!(body.join("\n\n")));
    content.insert("format".to_owned(), json!(HTML));
    content.insert("formatted_body".to_owned(), json!(html.join("\n")));
    content.insert("m.mentions".to_owned(), json!({ "user_ids": mentioned }));
    content.insert(ROUTE.to_owned(), json!(announcement.marks.route.as_str()));
    content.insert(
        MARKER.to_owned(),
        json!(announcement.marks.announcement.as_str()),
    );
    Value::Object(content)
}

fn chunk_content(announcement: &Announcement, index: u32, root: &str, before: &str) -> Value {
    let part = usize::try_from(index)
        .ok()
        .and_then(|index| announcement.parts.get(index));
    json!({
        "msgtype": "m.text",
        "body": part.map(|part| part.body.as_str()).unwrap_or_default(),
        "format": HTML,
        "formatted_body": part.and_then(|part| part.html.as_deref()).unwrap_or_default(),
        "m.mentions": {},
        "m.relates_to": {
            "rel_type": "m.thread",
            "event_id": root,
            "is_falling_back": true,
            "m.in_reply_to": { "event_id": before },
        },
    })
}

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

fn sent(matrix: &Matrix, transaction: &str, content: &Value) -> Request {
    let path = format!(
        "{}/send/m.room.message/{}",
        room(matrix),
        segment(transaction)
    );
    put(matrix, &path, content)
}

pub(crate) fn step(
    matrix: &Matrix,
    step: &Step,
    announcement: &Announcement,
    entry: &Entry,
) -> Result<Request, Refusal> {
    let key = announcement.key.to_string();
    match step {
        Step::Post => Ok(sent(matrix, &key, &announcement_content(announcement))),
        Step::Chunk(index) => {
            let root = posted(entry, &Step::Post, "the announcement's message")?;
            let before = match index.checked_sub(1).filter(|earlier| *earlier > 0) {
                None => root,
                Some(earlier) => posted(entry, &Step::Chunk(earlier), "the message before it")?,
            };
            let content = chunk_content(announcement, *index, root, before);
            Ok(sent(matrix, &format!("{key}.{index}"), &content))
        }
        Step::Pin | Step::Unpin => Ok(pinned(matrix)),
        Step::Crosspost => Err(Refusal::Cannot("crosspost")),
        Step::OpenThread => Err(Refusal::Cannot("open a thread; its replies are the thread")),
        Step::Deliver(_) => Err(Refusal::Cannot("deliver to an inbox")),
    }
}

/// The request reading the room's pinned events.
pub(crate) fn pinned(matrix: &Matrix) -> Request {
    get(matrix, &format!("{}/state/{PINNED_EVENTS}", room(matrix)))
}

/// The room's pinned events, as `response` holds them, or what the
/// homeserver's trouble means.
pub(crate) fn pins(response: &Response) -> Result<Vec<String>, Sent> {
    let body = json(response);
    if response.status == NOT_FOUND {
        return Ok(Vec::new());
    }
    if let Some(sent) = trouble(response, &body) {
        return Err(sent);
    }
    Ok(body
        .get("pinned")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect())
}

/// What a pin or unpin acts on, read before anything is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Repin<'a> {
    /// The announcement's message is pinned.
    Pin(&'a str),
    /// The route's earlier pins are unpinned, the announcement's own kept:
    /// the one the store remembers, or else each of herald's own pins that
    /// carries the route's marker (ADR-0011).
    Unpin {
        post: &'a str,
        remembered: Option<&'a str>,
    },
}

pub(crate) fn pin_target<'a>(
    step: &Step,
    announcement: &'a Announcement,
    entry: &'a Entry,
) -> Result<Repin<'a>, Refusal> {
    let post = posted(entry, &Step::Post, "the announcement's message")?;
    Ok(if *step == Step::Pin {
        Repin::Pin(post)
    } else {
        Repin::Unpin {
            post,
            remembered: announcement.replaces.as_deref(),
        }
    })
}

/// The room's pins with `post` added, where it is not pinned yet.
pub(crate) fn with(held: &[String], post: &str) -> Vec<String> {
    let mut pins = held.to_vec();
    if !pins.iter().any(|pinned| pinned == post) {
        pins.push(post.to_owned());
    }
    pins
}

/// The room's pins without `stale`, `post` kept whatever `stale` holds.
pub(crate) fn without(held: &[String], stale: &[String], post: &str) -> Vec<String> {
    held.iter()
        .filter(|pinned| *pinned == post || !stale.contains(pinned))
        .cloned()
        .collect()
}

/// The write setting the room's pins to `pins`.
pub(crate) fn repinned(matrix: &Matrix, pins: &[String]) -> Request {
    let path = format!("{}/state/{PINNED_EVENTS}", room(matrix));
    put(matrix, &path, &json!({ "pinned": pins }))
}

/// The request reading the event `event` of the room.
pub(crate) fn event(matrix: &Matrix, event: &str) -> Request {
    get(
        matrix,
        &format!("{}/event/{}", room(matrix), segment(event)),
    )
}
