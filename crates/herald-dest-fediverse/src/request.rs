use herald_core::{Announcement, Declined, Entry, Step};
use herald_ports::{Method, Request, Text, USER_AGENT};
use serde_json::{Map, Value, json};

use crate::mastodon::{Mastodon, PLATFORM};

/// Why Mastodon cannot take a step.
pub(crate) const fn cannot(what: &'static str) -> Declined {
    Declined::Cannot {
        platform: PLATFORM,
        what,
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
    let title = announcement
        .title
        .shown(crate::mastodon::MASTODON_MARKUP)
        .body;
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

pub(crate) fn step(
    mastodon: &Mastodon,
    step: &Step,
    announcement: &Announcement,
    entry: &Entry,
) -> Result<Request, Declined> {
    let key = announcement.key.to_string();
    match step {
        Step::Post => Ok(status(mastodon, &first(announcement), None, key)),
        Step::Chunk(index) => {
            let before = entry
                .before(*index)
                .ok_or(Declined::Missing(if *index > 1 {
                    "the status before it"
                } else {
                    "the announcement's status"
                }))?;
            let text = announcement
                .part(*index)
                .map(|part| part.body.as_str())
                .unwrap_or_default();
            Ok(status(
                mastodon,
                text,
                Some(before),
                format!("{key}.{index}"),
            ))
        }
        Step::Crosspost => Err(cannot("crosspost")),
        Step::OpenThread => Err(cannot("open a thread; its replies are the thread")),
        Step::Pin | Step::Unpin => Err(cannot("pin a route's announcement")),
        Step::Deliver(_) | Step::Deliveries => Err(cannot("deliver to an inbox")),
    }
}
