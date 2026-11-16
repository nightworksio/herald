//! A post brought to a new revision: each part replaced by an edit event,
//! and each chunk no longer needed redacted (ADR-0034).

use herald_core::{Announcement, Declined, Entry, NO_PART, Step, unreserved};
use herald_ports::Request;
use serde_json::{Value, json};

use crate::matrix::Matrix;
use crate::request::{announcement_content, chunk_content, posted, put, room, sent};

/// How Matrix names an event that replaces another.
const REPLACE: &str = "m.replace";

/// What marks an edit's fallback text as an edit, for clients that show
/// edits as messages of their own.
const EDIT_FALLBACK: &str = "* ";

/// The edit replacing the event of `part` with what `announcement` now
/// reads there, sent once per revision whose digest is `revision`.
pub(crate) fn edit(
    matrix: &Matrix,
    (revision, part): (&str, u32),
    announcement: &Announcement,
    entry: &Entry,
) -> Result<Request, Declined> {
    let target = entry.part(part).ok_or(NO_PART)?;
    let mut content = match part {
        0 => announcement_content(announcement),
        chunk => {
            let root = posted(entry, &Step::Post, "the announcement's message")?;
            let before = entry
                .before(chunk)
                .ok_or(Declined::Missing("the message before it"))?;
            chunk_content(announcement, chunk, root, before)
        }
    };
    if let Value::Object(fields) = &mut content {
        fields.remove("m.relates_to");
    }
    let fallback = |field: &str| {
        format!(
            "{EDIT_FALLBACK}{}",
            content
                .get(field)
                .and_then(Value::as_str)
                .unwrap_or_default()
        )
    };
    let edit = json!({
        "msgtype": "m.text",
        "body": fallback("body"),
        "format": content.get("format"),
        "formatted_body": fallback("formatted_body"),
        "m.mentions": {},
        "m.new_content": content,
        "m.relates_to": { "rel_type": REPLACE, "event_id": target },
    });
    let key = announcement.key.to_string();
    Ok(sent(matrix, &format!("{key}.{revision}.{part}"), &edit))
}

/// The redaction of the event of the overflow chunk `part`.
pub(crate) fn redact(
    matrix: &Matrix,
    part: u32,
    key: &str,
    entry: &Entry,
) -> Result<Request, Declined> {
    let target = entry.part(part).ok_or(NO_PART)?;
    let path = format!(
        "{}/redact/{}/{}",
        room(matrix),
        unreserved(target),
        unreserved(&format!("{key}.redact.{part}"))
    );
    Ok(put(matrix, &path, &json!({})))
}
