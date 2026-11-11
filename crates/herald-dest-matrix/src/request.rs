use herald_core::{Announcement, Declined, Entry, Sent, Step, Waits, safe_url};
use herald_ports::{Method, Request, Response, Text, USER_AGENT, unreserved};
use serde_json::{Map, Value, json};

use crate::matrix::{MATRIX, Matrix};
use crate::read::{json, trouble};

/// The field of a message's content carrying its route's marker (ADR-0011).
pub(crate) const ROUTE: &str = "io.nightworks.herald.route";
/// The field of a message's content carrying its announcement's marker
/// (ADR-0011).
pub(crate) const MARKER: &str = "io.nightworks.herald.marker";
/// The format of a message's sanitised HTML.
const HTML: &str = "org.matrix.custom.html";
/// Where a mention and a permalink lead, as Matrix clients read them.
pub(crate) const MATRIX_TO: &str = "https://matrix.to/";
/// Where the client-server API lies under the homeserver.
pub(crate) const CLIENT: &str = "/_matrix/client/v3";
/// The punctuation a user id's localpart may hold, the historical ones
/// included, that neither HTML nor a link's fragment reads.
const LOCALPART: &[u8] = b"!$()*+,-./;=?@_~";
/// The state event holding a room's pinned events.
pub(crate) const PINNED_EVENTS: &str = "m.room.pinned_events";
/// The event a message is.
pub(crate) const MESSAGE: &str = "m.room.message";
/// The answer of a homeserver for state a room does not hold.
pub(crate) const NOT_FOUND: u16 = 404;

/// Why Matrix cannot take a step.
pub(crate) const fn cannot(what: &'static str) -> Declined {
    Declined::Cannot {
        platform: MATRIX,
        what,
    }
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
    let url = format!("{}{CLIENT}{path}", matrix.homeserver);
    authorised(Request::new(Method::Get, Text::plain(url)), matrix)
}

fn put(matrix: &Matrix, path: &str, body: &Value) -> Request {
    let url = format!("{}{CLIENT}{path}", matrix.homeserver);
    authorised(Request::new(Method::Put, Text::plain(url)), matrix)
        .header("Content-Type", Text::plain("application/json"))
        .body(body.to_string())
}

pub(crate) fn room(matrix: &Matrix) -> String {
    format!("/rooms/{}", unreserved(&matrix.route.room))
}

/// Whether `user` is written as a Matrix user id, so it can be mentioned.
fn user_id(user: &str) -> bool {
    user.strip_prefix('@')
        .and_then(|rest| rest.split_once(':'))
        .is_some_and(|(local, server)| {
            !local.is_empty()
                && local
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || LOCALPART.contains(&byte))
                && !server.is_empty()
                && server
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b".-:[]".contains(&byte))
        })
}

fn announcement_content(announcement: &Announcement) -> Value {
    let first = announcement.parts.first();
    let title = announcement.title.shown(crate::matrix::MARKUP);
    let link = announcement
        .link
        .as_deref()
        .map(|link| announcement.marks.on(link));
    let mentioned = mentionable(&announcement.mentioned);
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
        let (text, pills) = mentions(&mentioned);
        body.push(text);
        html.push(pills);
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

/// The users of `users` written as Matrix user ids, which can be mentioned.
fn mentionable(users: &[String]) -> Vec<&str> {
    users
        .iter()
        .map(String::as_str)
        .filter(|user| user_id(user))
        .collect()
}

/// `users` mentioned: as the plain body writes them, and as pills.
fn mentions(users: &[&str]) -> (String, String) {
    let pills: Vec<String> = users
        .iter()
        .map(|user| format!("<a href=\"{MATRIX_TO}#/{user}\">{user}</a>"))
        .collect();
    (users.join(" "), format!("<p>{}</p>", pills.join(" ")))
}

fn chunk_content(announcement: &Announcement, index: u32, root: &str, before: &str) -> Value {
    let mentioned = usize::try_from(index)
        .ok()
        .and_then(|index| announcement.mentioned_in(index))
        .map(mentionable)
        .unwrap_or_default();
    let (body, html) = if mentioned.is_empty() {
        let part = announcement.part(index);
        (
            part.map(|part| part.body.clone()).unwrap_or_default(),
            part.and_then(|part| part.html.clone()).unwrap_or_default(),
        )
    } else {
        mentions(&mentioned)
    };
    let mentioning = if mentioned.is_empty() {
        json!({})
    } else {
        json!({ "user_ids": mentioned })
    };
    json!({
        "msgtype": "m.text",
        "body": body,
        "format": HTML,
        "formatted_body": html,
        "m.mentions": mentioning,
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
) -> Result<&'a str, Declined> {
    entry.message(step).ok_or(Declined::Missing(what))
}

fn sent(matrix: &Matrix, transaction: &str, content: &Value) -> Request {
    let path = format!(
        "{}/send/{MESSAGE}/{}",
        room(matrix),
        unreserved(transaction)
    );
    put(matrix, &path, content)
}

pub(crate) fn step(
    matrix: &Matrix,
    step: &Step,
    announcement: &Announcement,
    entry: &Entry,
) -> Result<Request, Declined> {
    let key = announcement.key.to_string();
    match step {
        Step::Post => Ok(sent(matrix, &key, &announcement_content(announcement))),
        Step::Chunk(index) => {
            let root = posted(entry, &Step::Post, "the announcement's message")?;
            let before = entry
                .before(*index)
                .ok_or(Declined::Missing("the message before it"))?;
            let content = chunk_content(announcement, *index, root, before);
            Ok(sent(matrix, &format!("{key}.{index}"), &content))
        }
        Step::Pin | Step::Unpin => Ok(pinned(matrix)),
        Step::Crosspost => Err(cannot("crosspost")),
        Step::OpenThread => Err(cannot("open a thread; its replies are the thread")),
        Step::Deliver(_) | Step::Deliveries => Err(cannot("deliver to an inbox")),
    }
}

/// The request reading the room's pinned events.
pub(crate) fn pinned(matrix: &Matrix) -> Request {
    get(matrix, &format!("{}/state/{PINNED_EVENTS}", room(matrix)))
}

/// The room's pinned events, as `response` holds them, or what the
/// homeserver's trouble means.
pub(crate) fn pins(response: &Response, waits: Waits) -> Result<Vec<String>, Sent> {
    if response.status == NOT_FOUND {
        return Ok(Vec::new());
    }
    if let Some(sent) = trouble(response, waits) {
        return Err(sent);
    }
    Ok(json(response)
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
) -> Result<Repin<'a>, Declined> {
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
        &format!("{}/event/{}", room(matrix), unreserved(event)),
    )
}
