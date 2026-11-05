//! Matrix's commands: what a member asked, read from the messages an
//! application service is pushed or a sync returns, and herald's answers as
//! the requests the homeserver takes (ADR-0016).

use std::fmt;

use herald_core::{Answering, Asked, Command, DestinationName, Text as Written, digest};
use herald_ports::{Method, Request, Response, Secret, Text};
use serde_json::{Value, json};

use crate::interactions::{Interactions, Via};
use crate::request::segment;

/// The word after a project that asks for the answer to be posted to the
/// room rather than in a thread.
const SHARE: &str = "share";
/// How long a sync waits for an event before it answers with none, in
/// milliseconds.
pub const SYNC_WAIT: u32 = 30_000;
/// The filter a sync reads messages with: room messages alone, with no
/// presence, account data or history before the sync began.
const FILTER: &str = r#"{"presence":{"types":[]},"account_data":{"types":[]},"room":{"timeline":{"types":["m.room.message"],"limit":20},"state":{"types":[]},"ephemeral":{"types":[]},"account_data":{"types":[]}}}"#;

/// How a Matrix destination takes and answers commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interacting {
    homeserver: String,
    interactions: Interactions,
    token: Secret,
}

/// Where herald's answer to a command goes: the room it was given in,
/// relating to the message that gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replying {
    /// The room.
    pub room: String,
    /// The message that gave the command.
    pub event: String,
}

/// What a sync returned: the commands given since the last, and where the
/// next sync starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Synced {
    /// Where the next sync starts.
    pub next: String,
    /// The commands given, each with where its answer goes.
    pub asked: Vec<(Asked, Replying)>,
}

/// Why a transaction or a sync could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread {
    /// The body is not a transaction or a sync's answer.
    NotEvents,
}

impl fmt::Display for Unread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the body is not a Matrix transaction or sync")
    }
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

impl Interacting {
    /// How the destination on `homeserver` with `interactions` takes
    /// commands, acting with `token`: the application service's own where
    /// the homeserver pushes to herald, and the bot's where herald syncs.
    #[must_use]
    pub fn new(homeserver: &str, interactions: Interactions, token: Secret) -> Self {
        Self {
            homeserver: homeserver.trim_end_matches('/').to_owned(),
            interactions,
            token,
        }
    }

    /// The homeserver's base URL.
    #[must_use]
    pub fn api(&self) -> &str {
        &self.homeserver
    }

    /// How it carries commands.
    #[must_use]
    pub fn via(&self) -> Via {
        self.interactions.via
    }

    /// The application service's user, by its localpart.
    #[must_use]
    pub fn sender(&self) -> &str {
        &self.interactions.sender
    }

    /// How it answers, whatever carries the commands.
    #[must_use]
    pub fn answering(&self) -> Answering {
        self.interactions.answering()
    }

    /// The commands in the transaction `body` the homeserver pushed to
    /// `destination`, each with where its answer goes.
    ///
    /// # Errors
    ///
    /// When it is not a transaction.
    pub fn transaction(
        &self,
        destination: &DestinationName,
        body: &[u8],
    ) -> Result<Vec<(Asked, Replying)>, Unread> {
        let value: Value = serde_json::from_slice(body).map_err(|_| Unread::NotEvents)?;
        let events = value
            .get("events")
            .and_then(Value::as_array)
            .ok_or(Unread::NotEvents)?;
        Ok(events
            .iter()
            .filter_map(|event| self.command(destination, text(event, "room_id")?, event))
            .collect())
    }

    /// The commands in the sync answer `body`, read for `destination`, and
    /// where the next sync starts.
    ///
    /// # Errors
    ///
    /// When it is not a sync's answer.
    pub fn synced(&self, destination: &DestinationName, body: &[u8]) -> Result<Synced, Unread> {
        let value: Value = serde_json::from_slice(body).map_err(|_| Unread::NotEvents)?;
        let next = text(&value, "next_batch")
            .ok_or(Unread::NotEvents)?
            .to_owned();
        let asked = value
            .pointer("/rooms/join")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .flat_map(|(room, joined)| {
                joined
                    .pointer("/timeline/events")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|event| self.command(destination, room, event))
            })
            .collect();
        Ok(Synced { next, asked })
    }

    fn command(
        &self,
        destination: &DestinationName,
        room: &str,
        event: &Value,
    ) -> Option<(Asked, Replying)> {
        if text(event, "type") != Some("m.room.message") {
            return None;
        }
        let content = event.get("content")?;
        if text(content, "msgtype") != Some("m.text") {
            return None;
        }
        let sender = text(event, "sender")?;
        if sender
            .strip_prefix('@')
            .and_then(|user| user.split_once(':'))
            .is_some_and(|(local, _)| local == self.interactions.sender)
        {
            return None;
        }
        let mut words = text(content, "body")?.split_whitespace();
        if words.next() != Some(self.interactions.prefix.as_str()) {
            return None;
        }
        let command = Command::named(words.next()?)?;
        let rest: Vec<&str> = words.collect();
        let (project, share) = match rest.as_slice() {
            [] => (None, false),
            [word] if word.eq_ignore_ascii_case(SHARE) => (None, true),
            [project] => (Some(*project), false),
            [project, word] if word.eq_ignore_ascii_case(SHARE) => (Some(*project), true),
            _ => return None,
        };
        Some((
            Asked {
                destination: destination.clone(),
                place: room.to_owned(),
                user: sender.to_owned(),
                command,
                project: project.map(ToOwned::to_owned),
                share,
                toggles: false,
                member: None,
            },
            Replying {
                room: room.to_owned(),
                event: text(event, "event_id")?.to_owned(),
            },
        ))
    }

    fn authorised(&self, request: Request) -> Request {
        request
            .header("User-Agent", Text::plain(herald_ports::USER_AGENT))
            .header(
                "Authorization",
                Text::plain("Bearer ").then_secret(self.token.clone()),
            )
    }

    /// The request for the messages given since `since`, or for where the
    /// next sync starts where it names none, waiting up to `wait`
    /// milliseconds for one.
    #[must_use]
    pub fn sync(&self, since: Option<&str>, wait: u32) -> Request {
        let mut url = format!(
            "{}/_matrix/client/v3/sync?timeout={wait}&filter={}",
            self.homeserver,
            segment(FILTER)
        );
        if let Some(since) = since {
            url.push_str("&since=");
            url.push_str(&segment(since));
        }
        self.authorised(Request::new(Method::Get, Text::plain(url)))
    }

    /// The requests that answer the command `replying` names with `parts`:
    /// in a thread under it unless `public`, else in the room as a reply to
    /// it. Each is sent once whatever retries it, by a transaction id drawn
    /// from the command and the part.
    #[must_use]
    pub fn answered(&self, replying: &Replying, parts: &[Written], public: bool) -> Vec<Request> {
        let relates = if public {
            json!({ "m.in_reply_to": { "event_id": replying.event } })
        } else {
            json!({
                "rel_type": "m.thread",
                "event_id": replying.event,
                "is_falling_back": true,
                "m.in_reply_to": { "event_id": replying.event },
            })
        };
        let drawn: String = digest(&replying.event).chars().take(32).collect();
        parts
            .iter()
            .enumerate()
            .map(|(index, part)| {
                let mut content = json!({
                    "msgtype": "m.notice",
                    "body": part.body,
                    "m.mentions": {},
                    "m.relates_to": relates,
                });
                if let (Some(html), Some(object)) = (&part.html, content.as_object_mut()) {
                    object.insert("format".to_owned(), json!("org.matrix.custom.html"));
                    object.insert("formatted_body".to_owned(), json!(html));
                }
                let url = format!(
                    "{}/_matrix/client/v3/rooms/{}/send/m.room.message/herald.{drawn}.{index}",
                    self.homeserver,
                    segment(&replying.room)
                );
                self.authorised(Request::new(Method::Put, Text::plain(url)))
                    .header("Content-Type", Text::plain("application/json"))
                    .body(content.to_string())
            })
            .collect()
    }

    /// The registration the homeserver's administrator installs: herald at
    /// `url`, with the tokens `homeserver` and `appservice`, owning its
    /// sender alone.
    #[must_use]
    pub fn registration(&self, url: &str, homeserver: &str, appservice: &str) -> String {
        let sender = &self.interactions.sender;
        let escaped: String = sender
            .chars()
            .flat_map(|character| {
                let special = matches!(character, '.' | '/' | '+' | '=' | '-');
                special
                    .then_some('\\')
                    .into_iter()
                    .chain(std::iter::once(character))
            })
            .collect();
        [
            "id: 'herald'".to_owned(),
            format!("url: {}", quoted(url)),
            format!("as_token: {}", quoted(appservice)),
            format!("hs_token: {}", quoted(homeserver)),
            format!("sender_localpart: {}", quoted(sender)),
            "rate_limited: false".to_owned(),
            "namespaces:".to_owned(),
            "  users:".to_owned(),
            "    - exclusive: true".to_owned(),
            format!("      regex: {}", quoted(&format!("@{escaped}:.*"))),
            "  rooms: []".to_owned(),
            "  aliases: []".to_owned(),
            String::new(),
        ]
        .join("\n")
    }
}

/// `value` as a single-quoted YAML scalar, in which nothing escapes but a
/// quote, written twice.
fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Whether the homeserver took an answer, or else why not.
///
/// # Errors
///
/// What the homeserver said, where it refused or was unwell.
pub fn took(response: &Response) -> Result<(), String> {
    if response.succeeded() {
        return Ok(());
    }
    let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    let said = [text(&body, "errcode"), text(&body, "error")]
        .into_iter()
        .flatten()
        .collect::<Vec<&str>>()
        .join(" ");
    Err(
        format!("the homeserver answered {}: {said}", response.status)
            .trim_end_matches([' ', ':'])
            .to_owned(),
    )
}

#[cfg(test)]
mod tests;
