use herald_core::{Announcement, Declined, Step};
use herald_ports::{Method, Request, Secret, SecretText};
use serde_json::Value;

use crate::card::{Measured, Offers, announced, chunk};
use crate::teams::MESSAGE;

/// What a Workflows webhook is, as a refusal names it.
const WEBHOOK: &str = "A Microsoft Teams Workflows webhook";

/// The content type of each request's JSON body.
pub(crate) const JSON: &str = "application/json; charset=utf-8";

const fn cannot(what: &'static str) -> Declined {
    Declined::Cannot {
        platform: WEBHOOK,
        what,
    }
}

/// The post of `card` to the webhook `url`, where it fits what the webhook
/// takes.
fn posted(url: &Secret, card: &Value) -> Result<Request, String> {
    let body = Measured::Bytes(MESSAGE).fitted(card.to_string())?;
    Ok(Request::new(Method::Post, SecretText::secret(url.clone()))
        .identified()
        .header("Content-Type", SecretText::plain(JSON))
        .body(body))
}

/// The request for `step` of `announcement` to the webhook `url`, or why a
/// webhook cannot take it.
pub(crate) fn request(
    url: &Secret,
    step: &Step,
    announcement: &Announcement,
) -> Result<Request, String> {
    let declined = match step {
        Step::Post => return posted(url, &announced(announcement, Offers::NONE)),
        Step::Chunk(index) => return posted(url, &chunk(announcement, *index, Offers::NONE)),
        Step::Crosspost => cannot("crosspost"),
        Step::OpenThread | Step::Rename(_) => cannot("open a thread"),
        Step::Pin | Step::Unpin => cannot("pin"),
        Step::Edit { .. } => cannot("edit a message; a bot, given app_id and app_password, can"),
        Step::Delete(_) => cannot("delete a message; a bot, given app_id and app_password, can"),
        Step::Deliver(_) | Step::Deliveries | Step::Updates(_) | Step::Deletes => {
            cannot("deliver to an inbox")
        }
    };
    Err(declined.to_string())
}
