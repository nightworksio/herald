//! The Adaptive Cards herald posts, in the message activity both a
//! Workflows webhook and the Bot Framework take.

use herald_core::{Announcement, Wording, safe_url};
use serde_json::{Map, Value, json};

use crate::interact::{EXECUTE, SUBSCRIBE};
use crate::options::guid;
use crate::teams::{MARKUP, TEAMS};

/// The content type of an Adaptive Card attachment.
const CARD: &str = "application/vnd.microsoft.card.adaptive";
/// The schema an Adaptive Card names.
const SCHEMA: &str = "http://adaptivecards.io/schemas/adaptive-card.json";
/// The lowest Adaptive Card version holding every element and property a
/// card of herald's uses: `TextBlock` with `size`, `weight` and `wrap`,
/// and `Action.OpenUrl` (<https://github.com/microsoft/AdaptiveCards/blob/main/schemas/1.1.0/adaptive-card.json>
/// marks none of them newer than 1.0).
const VERSION: &str = "1.0";
/// The Adaptive Card version a card holding an `Action.Execute` names:
/// Teams takes the action from cards of 1.5
/// (<https://learn.microsoft.com/en-us/microsoftteams/platform/task-modules-and-cards/cards/universal-actions-for-adaptive-cards/work-with-universal-actions-for-adaptive-cards>,
/// "Schema for Universal Actions for Adaptive Cards").
const EXECUTING: &str = "1.5";

/// What a bot's cards carry beyond an announcement: the label its mentions
/// show, where it mentions, and the subscribe button's label, where its
/// route offers one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Offers<'a> {
    pub(crate) mentioned: Option<&'a str>,
    pub(crate) button: Option<&'a str>,
}

impl Offers<'_> {
    /// What a card carries where it mentions no one and offers nothing.
    pub(crate) const NONE: Offers<'static> = Offers {
        mentioned: None,
        button: None,
    };
}

/// How a message's size is counted against what it may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Measured {
    /// In bytes, as a Workflows webhook counts it.
    Bytes(usize),
    /// In bytes of its UTF-16 encoding, as the Bot Framework counts it.
    Utf16(usize),
}

impl Measured {
    /// `body`, where it fits; why not otherwise.
    pub(crate) fn fitted(self, body: String) -> Result<String, String> {
        let (size, most, unit) = match self {
            Self::Bytes(most) => (body.len(), most, "bytes"),
            Self::Utf16(most) => (
                body.encode_utf16().count().saturating_mul(2),
                most,
                "bytes as UTF-16",
            ),
        };
        if size > most {
            return Err(format!(
                "the card is {size} {unit}, more than the {most} {TEAMS} takes in a message"
            ));
        }
        Ok(body)
    }
}

fn text(text: &str) -> Value {
    json!({ "type": "TextBlock", "text": text, "wrap": true })
}

/// The mentions of `users` as card text and its entities, each labelled
/// `label` and numbered, where any is a Microsoft Entra object id.
fn mentioning(users: &[String], label: &str) -> Option<(Value, Vec<Value>)> {
    let (written, entities): (Vec<String>, Vec<Value>) = users
        .iter()
        .filter(|user| guid(user))
        .zip(1..)
        .map(|(user, number)| {
            let name = format!("{label} {number}");
            let text = format!("<at>{name}</at>");
            let entity = json!({
                "type": "mention",
                "text": text,
                "mentioned": { "id": user, "name": name },
            });
            (text, entity)
        })
        .unzip();
    (!written.is_empty()).then(|| (text(&written.join(" ")), entities))
}

/// The message holding one Adaptive Card of `body`, `actions` and the
/// mention `entities`.
fn card(body: &[Value], actions: Vec<Value>, entities: Vec<Value>) -> Value {
    let executes = actions
        .iter()
        .any(|action| action.get("type").and_then(Value::as_str) == Some(EXECUTE));
    let mut content = Map::new();
    content.insert("$schema".to_owned(), json!(SCHEMA));
    content.insert("type".to_owned(), json!("AdaptiveCard"));
    content.insert(
        "version".to_owned(),
        json!(if executes { EXECUTING } else { VERSION }),
    );
    content.insert("body".to_owned(), Value::Array(body.to_vec()));
    if !actions.is_empty() {
        content.insert("actions".to_owned(), Value::Array(actions));
    }
    if !entities.is_empty() {
        content.insert(
            "msteams".to_owned(),
            json!({ "entities": Value::Array(entities) }),
        );
    }
    json!({
        "type": "message",
        "attachments": [{ "contentType": CARD, "contentUrl": null, "content": content }],
    })
}

/// The card of the announcement itself: its title, its first part, a
/// button to each of its kind's links, or else its link shown, the members
/// it mentions, and the subscribe button where `offers` holds one.
pub(crate) fn announced(announcement: &Announcement, offers: Offers<'_>) -> Value {
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
    let mut actions: Vec<Value> = announcement
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
    let mut entities = Vec::new();
    if let Some((mentions, mentioned)) = offers
        .mentioned
        .and_then(|label| mentioning(&announcement.mentioned, label))
    {
        body.push(mentions);
        entities = mentioned;
    }
    actions.extend(offers.button.map(|label| {
        json!({
            "type": EXECUTE,
            "title": label,
            "verb": SUBSCRIBE,
            "data": { "project": announcement.project },
        })
    }));
    card(&body, actions, entities)
}

/// The card of the overflow message `index`: one of the announcement's
/// parts, or else the members mentioned after them.
pub(crate) fn chunk(announcement: &Announcement, index: u32, offers: Offers<'_>) -> Value {
    let after = usize::try_from(index)
        .ok()
        .and_then(|index| announcement.mentioned_in(index));
    if let Some(users) = after {
        let (body, entities) = offers
            .mentioned
            .and_then(|label| mentioning(users, label))
            .unwrap_or_else(|| (text(""), Vec::new()));
        return card(&[body], Vec::new(), entities);
    }
    let part = announcement
        .part(index)
        .map(|part| part.body.as_str())
        .unwrap_or_default();
    card(&[text(part)], Vec::new(), Vec::new())
}

/// The card of one part of herald's answer to a command.
pub(crate) fn answer(part: &str) -> Value {
    card(&[text(part)], Vec::new(), Vec::new())
}

/// The card of part `part`: the announcement's for part 0, else the
/// overflow message's.
pub(crate) fn of_part(announcement: &Announcement, part: u32, offers: Offers<'_>) -> Value {
    match part {
        0 => announced(announcement, offers),
        index => chunk(announcement, index, offers),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_measured_as_its_way_counts_it() {
        let ascii = "a".repeat(10);
        assert_eq!(Measured::Bytes(10).fitted(ascii.clone()), Ok(ascii.clone()));
        assert_eq!(Measured::Utf16(20).fitted(ascii.clone()), Ok(ascii.clone()));
        assert_eq!(
            Measured::Utf16(19).fitted(ascii.clone()),
            Err(format!(
                "the card is 20 bytes as UTF-16, more than the 19 {TEAMS} takes in a message"
            ))
        );
        assert_eq!(
            Measured::Bytes(9).fitted(ascii),
            Err(format!(
                "the card is 10 bytes, more than the 9 {TEAMS} takes in a message"
            ))
        );
        let wide = "é😀".to_owned();
        assert_eq!(Measured::Bytes(6).fitted(wide.clone()), Ok(wide.clone()));
        assert_eq!(Measured::Utf16(6).fitted(wide.clone()), Ok(wide.clone()));
        assert!(Measured::Utf16(5).fitted(wide).is_err());
    }
}
