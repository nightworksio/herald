use std::fmt;

use herald_core::{Announcement, Dialect, Entry, Step};
use herald_ports::{Method, Request, Text, USER_AGENT};
use serde_json::{Map, Value, json};

use crate::mastodon::Mastodon;

/// Why a step cannot be asked of Mastodon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Mastodon has no such thing to do.
    Cannot(&'static str),
    /// The step needs an earlier step's answer, which is not recorded.
    Missing(&'static str),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cannot(what) => write!(f, "Mastodon cannot {what}"),
            Self::Missing(what) => write!(
                f,
                "{what} is not recorded, so this step cannot be taken yet"
            ),
        }
    }
}

pub(crate) fn get(mastodon: &Mastodon, path: &str) -> Request {
    let url = format!("{}{path}", mastodon.server);
    authorised(Request::new(Method::Get, Text::plain(url)), mastodon)
}

fn authorised(request: Request, mastodon: &Mastodon) -> Request {
    request
        .header("User-Agent", Text::plain(USER_AGENT))
        .header(
            "Authorization",
            Text::plain("Bearer ").then_secret(mastodon.token.clone()),
        )
}

fn status(mastodon: &Mastodon, text: &str, reply_to: Option<&str>, key: String) -> Request {
    let route = &mastodon.route;
    let mut body = Map::new();
    body.insert("status".to_owned(), json!(text));
    body.insert("visibility".to_owned(), json!(route.visibility.as_str()));
    body.extend(
        route
            .language
            .iter()
            .map(|language| ("language".to_owned(), json!(language))),
    );
    body.extend(
        route
            .content_warning
            .iter()
            .map(|warning| ("spoiler_text".to_owned(), json!(warning))),
    );
    body.extend(reply_to.map(|reply_to| ("in_reply_to_id".to_owned(), json!(reply_to))));
    let url = Text::plain(format!("{}/api/v1/statuses", mastodon.server));
    authorised(Request::new(Method::Post, url), mastodon)
        .header("Content-Type", Text::plain("application/json"))
        .header("Idempotency-Key", Text::plain(key))
        .body(Value::Object(body).to_string())
}

fn first(announcement: &Announcement) -> String {
    let title = announcement.title.shown(Dialect::Plain).body;
    let text = announcement.parts.first().map(|part| part.body.as_str());
    let link = announcement
        .link
        .as_deref()
        .map(|link| announcement.marks.on(link));
    [Some(title.as_str()), text, link.as_deref()]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn posted<'a>(entry: &'a Entry, step: &Step, what: &'static str) -> Result<&'a str, Refusal> {
    entry
        .facts(step)
        .and_then(|facts| facts.message.as_deref())
        .ok_or(Refusal::Missing(what))
}

pub(crate) fn step(
    mastodon: &Mastodon,
    step: &Step,
    announcement: &Announcement,
    entry: &Entry,
) -> Result<Request, Refusal> {
    let key = announcement.key.to_string();
    match step {
        Step::Post => Ok(status(mastodon, &first(announcement), None, key)),
        Step::Chunk(index) => {
            let before = match index.checked_sub(1).filter(|earlier| *earlier > 0) {
                None => posted(entry, &Step::Post, "the announcement's status")?,
                Some(earlier) => posted(entry, &Step::Chunk(earlier), "the status before it")?,
            };
            let text = usize::try_from(*index)
                .ok()
                .and_then(|index| announcement.parts.get(index))
                .map(|part| part.body.as_str())
                .unwrap_or_default();
            Ok(status(
                mastodon,
                text,
                Some(before),
                format!("{key}.{index}"),
            ))
        }
        Step::Crosspost => Err(Refusal::Cannot("crosspost")),
        Step::OpenThread => Err(Refusal::Cannot("open a thread; its replies are the thread")),
        Step::Pin | Step::Unpin => Err(Refusal::Cannot("pin a route's announcement")),
        Step::Deliver(_) | Step::Deliveries => Err(Refusal::Cannot("deliver to an inbox")),
    }
}
