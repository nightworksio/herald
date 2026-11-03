use herald_core::{Facts, Sent};
use herald_ports::Response;
use jiff::SignedDuration;
use serde_json::Value;

/// How long to wait when Slack is unwell or limits herald and says nothing
/// of when to ask again.
const OUTAGE: SignedDuration = SignedDuration::from_secs(5);
const TOO_MANY: u16 = 429;
const SERVER: u16 = 500;
/// The most of an answer that is not JSON a refusal quotes.
const QUOTED: usize = 200;

/// The JSON `response` holds, or else its text as a string.
pub(crate) fn json(response: &Response) -> Value {
    serde_json::from_slice(&response.body)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&response.body).into_owned()))
}

pub(crate) fn text<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

/// What a failed answer means: when to ask again, or why Slack refused.
/// Slack's Web API answers a refusal with `200` and `"ok": false`; an
/// incoming webhook answers one with a status and a word.
pub(crate) fn trouble(response: &Response, body: &Value) -> Option<Sent> {
    if response.status == TOO_MANY || response.status >= SERVER {
        let after = response
            .header("retry-after")
            .and_then(|after| after.parse::<u32>().ok())
            .map(|seconds| SignedDuration::from_secs(i64::from(seconds)));
        return Some(Sent::RetryAfter(after.unwrap_or(OUTAGE)));
    }
    if !response.succeeded() {
        let said: String = body
            .as_str()
            .map_or_else(
                || text(body, "error").unwrap_or_default().to_owned(),
                ToOwned::to_owned,
            )
            .trim()
            .chars()
            .take(QUOTED)
            .collect();
        let status = response.status;
        return Some(Sent::Refused(if said.is_empty() {
            format!("Slack refused it with {status}")
        } else {
            format!("Slack refused it with {status}: {said}")
        }));
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
pub(crate) fn read(response: &Response) -> Sent {
    let body = json(response);
    if let Some(sent) = trouble(response, &body) {
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
