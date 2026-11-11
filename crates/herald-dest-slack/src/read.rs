use herald_core::{Facts, Sent, Waits};
use herald_ports::Response;
use serde_json::Value;

use crate::slack::SLACK;
/// The most of an answer that is not JSON a refusal quotes.
const QUOTED: usize = 200;

/// The JSON `response` holds, or else its text as a string.
pub(crate) fn json(response: &Response) -> Value {
    serde_json::from_slice(&response.body)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&response.body).into_owned()))
}

/// `said` trimmed, and cut to the most of an answer a refusal quotes.
pub(crate) fn quoted(said: &str) -> String {
    said.trim().chars().take(QUOTED).collect()
}

pub(crate) fn text<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

/// What Slack said in `body`: the word an incoming webhook answers with,
/// or the Web API's error.
fn said(body: &Value) -> String {
    quoted(
        body.as_str()
            .unwrap_or_else(|| text(body, "error").unwrap_or_default()),
    )
}

/// What a failed answer means: when to ask again, or why Slack refused.
/// Slack's Web API answers a refusal with `200` and `"ok": false`; an
/// incoming webhook answers one with a status and a word.
pub(crate) fn trouble(response: &Response, body: &Value, waits: Waits) -> Option<Sent> {
    if let Some(sent) = response.trouble(waits, None, SLACK, || said(body)) {
        return Some(sent);
    }
    if body.get("ok").and_then(Value::as_bool) == Some(false) {
        let error = text(body, "error").unwrap_or("an unnamed error");
        return Some(Sent::Refused(match text(body, "needed") {
            Some(needed) => format!("Slack refused it: {error}, as the token lacks {needed}"),
            None => format!("Slack refused it: {error}"),
        }));
    }
    None
}

/// What a message posted answers: its timestamp, as Slack names a message,
/// where a bot posted it, or nothing more from a webhook.
pub(crate) fn read(response: &Response, waits: Waits) -> Sent {
    let body = json(response);
    if let Some(sent) = trouble(response, &body, waits) {
        return sent;
    }
    if body.is_string() {
        return Sent::Landed(Facts::default());
    }
    text(&body, "ts").map_or_else(
        || Sent::Refused("Slack's answer holds no message timestamp".to_owned()),
        |ts| {
            Sent::Landed(Facts {
                message: Some(ts.to_owned()),
                thread: None,
                permalink: None,
            })
        },
    )
}
