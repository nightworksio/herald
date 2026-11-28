//! The Adaptive Cards herald posts, in the message activity both a
//! Workflows webhook and the Bot Framework take.

use herald_core::{Announcement, Wording, safe_url};
use serde_json::{Value, json};

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
pub(crate) fn announced(announcement: &Announcement) -> Value {
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
pub(crate) fn chunk(announcement: &Announcement, index: u32) -> Value {
    let part = announcement
        .part(index)
        .map(|part| part.body.as_str())
        .unwrap_or_default();
    card(&[text(part)], Vec::new())
}

/// The card of part `part`: the announcement's for part 0, else the
/// overflow part's.
pub(crate) fn of_part(announcement: &Announcement, part: u32) -> Value {
    match part {
        0 => announced(announcement),
        index => chunk(announcement, index),
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
