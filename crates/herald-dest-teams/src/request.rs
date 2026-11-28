use herald_core::{Announcement, Declined, Step, Wording, safe_url};
use herald_ports::{Method, Request, SecretText};
use serde_json::{Value, json};

use crate::teams::{MARKUP, MESSAGE, TEAMS, Teams};

/// What a Workflows webhook is, as a refusal names it.
const WEBHOOK: &str = "A Microsoft Teams Workflows webhook";
/// The content type of an Adaptive Card attachment.
const CARD: &str = "application/vnd.microsoft.card.adaptive";
/// The schema an Adaptive Card names.
const SCHEMA: &str = "http://adaptivecards.io/schemas/adaptive-card.json";
/// The lowest Adaptive Card version holding every element and property a
/// card of herald's uses: `TextBlock` with `size`, `weight` and `wrap`,
/// and `Action.OpenUrl` (<https://github.com/microsoft/AdaptiveCards/blob/main/schemas/1.1.0/adaptive-card.json>
/// marks none of them newer than 1.0).
const VERSION: &str = "1.0";

const fn cannot(what: &'static str) -> Declined {
    Declined::Cannot {
        platform: WEBHOOK,
        what,
    }
}

fn text(text: &str) -> Value {
    json!({ "type": "TextBlock", "text": text, "wrap": true })
}

/// The message holding one Adaptive Card of `body` and `actions`.
fn card(body: &[Value], actions: Vec<Value>) -> Value {
    let mut content = json!({
        "$schema": SCHEMA,
        "type": "AdaptiveCard",
        "version": VERSION,
        "body": body,
    });
    if !actions.is_empty()
        && let Some(fields) = content.as_object_mut()
    {
        fields.insert("actions".to_owned(), Value::Array(actions));
    }
    json!({
        "type": "message",
        "attachments": [{ "contentType": CARD, "contentUrl": null, "content": content }],
    })
}

/// The card of the announcement itself: its title, its first part, and a
/// button to each of its kind's links, or else its link shown.
fn announced(announcement: &Announcement) -> Value {
    let wording = Wording::default();
    let mut body = vec![json!({
        "type": "TextBlock",
        "text": announcement.title.shown(MARKUP).body,
        "size": "Large",
        "weight": "Bolder",
        "wrap": true,
    })];
    body.extend(
        announcement
            .parts
            .first()
            .filter(|part| !part.body.is_empty())
            .map(|part| text(&part.body)),
    );
    let actions: Vec<Value> = announcement
        .buttons
        .iter()
        .map(|button| {
            json!({ "type": "Action.OpenUrl", "title": button.label(&wording), "url": button.url })
        })
        .collect();
    if actions.is_empty() {
        body.extend(announcement.link.as_deref().map(|link| {
            let shown = safe_url(link);
            text(&format!("[{shown}]({shown})"))
        }));
    }
    card(&body, actions)
}

/// The card of the overflow part `index`.
fn chunk(announcement: &Announcement, index: u32) -> Value {
    let part = announcement
        .part(index)
        .map(|part| part.body.as_str())
        .unwrap_or_default();
    card(&[text(part)], Vec::new())
}

/// The post of `card` to the webhook, where it fits what the webhook
/// takes.
fn posted(teams: &Teams, card: &Value) -> Result<Request, String> {
    let body = card.to_string();
    if body.len() > MESSAGE {
        return Err(format!(
            "the card is {} bytes, more than the {MESSAGE} {TEAMS} takes in a message",
            body.len()
        ));
    }
    Ok(
        Request::new(Method::Post, SecretText::secret(teams.webhook.clone()))
            .identified()
            .header(
                "Content-Type",
                SecretText::plain("application/json; charset=utf-8"),
            )
            .body(body),
    )
}

/// The request for `step` of `announcement`, or why a webhook cannot take
/// it.
pub(crate) fn request(
    teams: &Teams,
    step: &Step,
    announcement: &Announcement,
) -> Result<Request, String> {
    let declined = match step {
        Step::Post => return posted(teams, &announced(announcement)),
        Step::Chunk(index) => return posted(teams, &chunk(announcement, *index)),
        Step::Crosspost => cannot("crosspost"),
        Step::OpenThread | Step::Rename(_) => cannot("open a thread"),
        Step::Pin | Step::Unpin => cannot("pin"),
        Step::Edit { .. } => cannot("edit a message"),
        Step::Delete(_) => cannot("delete a message"),
        Step::Deliver(_) | Step::Deliveries | Step::Updates(_) | Step::Deletes => {
            cannot("deliver to an inbox")
        }
    };
    Err(declined.to_string())
}
