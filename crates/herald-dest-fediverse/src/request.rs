use herald_core::media;
use herald_core::{Announcement, Declined, Entry, NO_PART, Step};
use herald_ports::{Method, Request, SecretText};
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
    authorised(Request::new(Method::Get, SecretText::plain(url)), mastodon)
}

fn authorised(request: Request, mastodon: &Mastodon) -> Request {
    request.identified().bearer(mastodon.token.clone())
}

/// What a status reads, `text`, in the route's language and behind its
/// content warning.
fn written(mastodon: &Mastodon, text: &str) -> Map<String, Value> {
    let route = &mastodon.route;
    let mut body = Map::new();
    body.insert("status".to_owned(), json!(text));
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
    body
}

fn status(mastodon: &Mastodon, text: &str, reply_to: Option<&str>, key: String) -> Request {
    let mut body = written(mastodon, text);
    body.insert(
        "visibility".to_owned(),
        json!(mastodon.route.visibility.as_str()),
    );
    body.extend(reply_to.map(|reply_to| ("in_reply_to_id".to_owned(), json!(reply_to))));
    let url = SecretText::plain(format!("{}/api/v1/statuses", mastodon.server));
    authorised(Request::new(Method::Post, url), mastodon)
        .header("Content-Type", SecretText::plain(media::JSON))
        .header("Idempotency-Key", SecretText::plain(key))
        .body(Value::Object(body).to_string())
}

/// The longest status or account id herald takes from a server.
const ID: usize = 64;

/// Why a recorded status is not used: it is not shaped as an id.
const MALFORMED_STATUS: Declined = Declined::Malformed("Mastodon status id");

/// `id`, where it is shaped as a Mastodon API server's status or account
/// id: one to 64 ASCII letters, digits, `-` or `_`, which covers
/// Mastodon's numbers, the flake ids of Pleroma and Akkoma, and the ULIDs
/// of `GoToSocial`.
pub(crate) fn shaped(id: &str) -> Option<&str> {
    let fits = !id.is_empty()
        && id.len() <= ID
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    fits.then_some(id)
}

/// The status of `part` in the record `entry` keeps, addressed.
fn statused(mastodon: &Mastodon, entry: &Entry, part: u32) -> Result<SecretText, Declined> {
    let id = entry.part(part).ok_or(NO_PART)?;
    let id = shaped(id)
        .and_then(herald_core::segment)
        .ok_or(MALFORMED_STATUS)?;
    Ok(SecretText::plain(format!(
        "{}/api/v1/statuses/{id}",
        mastodon.server
    )))
}

/// What `part` of `announcement` reads.
fn text_of(announcement: &Announcement, part: u32) -> String {
    match part {
        0 => first(announcement),
        chunk => announcement
            .part(chunk)
            .map(|part| part.body.clone())
            .unwrap_or_default(),
    }
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
    match step {
        Step::Post => Ok(status(
            mastodon,
            &text_of(announcement, 0),
            None,
            entry.post_token(),
        )),
        Step::Chunk(index) => {
            let before = entry
                .before(*index)
                .ok_or(Declined::Missing(if *index > 1 {
                    "the status before it"
                } else {
                    "the announcement's status"
                }))?;
            Ok(status(
                mastodon,
                &text_of(announcement, *index),
                Some(before),
                entry.chunk_token(*index),
            ))
        }
        Step::Crosspost => Err(cannot("crosspost")),
        Step::OpenThread => Err(cannot("open a thread; its replies are the thread")),
        Step::Pin | Step::Unpin => Err(cannot("pin a route's announcement")),
        Step::Deliver(_) | Step::Deliveries | Step::Updates(_) | Step::Deletes => {
            Err(cannot("deliver to an inbox"))
        }
        Step::Edit { part, .. } => {
            let body = Value::Object(written(mastodon, &text_of(announcement, *part)));
            Ok(authorised(
                Request::new(Method::Put, statused(mastodon, entry, *part)?),
                mastodon,
            )
            .header("Content-Type", SecretText::plain(media::JSON))
            .body(body.to_string()))
        }
        Step::Delete(part) => Ok(authorised(
            Request::new(Method::Delete, statused(mastodon, entry, *part)?),
            mastodon,
        )),
        Step::Rename(_) => Err(cannot("rename a thread; its replies are the thread")),
    }
}
