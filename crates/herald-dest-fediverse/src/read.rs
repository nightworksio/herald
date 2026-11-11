use herald_core::{Facts, Sent, Waits};
use herald_ports::Response;
use jiff::Timestamp;
use serde_json::Value;

use crate::mastodon::PLATFORM;

pub(crate) fn text(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

/// The JSON `response` holds, or null where it holds none.
pub(crate) fn json(response: &Response) -> Value {
    serde_json::from_slice(&response.body).unwrap_or(Value::Null)
}

/// What a failed response means: when to ask again, or why the server
/// refused.
pub(crate) fn trouble(response: &Response, waits: Waits) -> Option<Sent> {
    let reset = response
        .header("x-ratelimit-reset")
        .and_then(|reset| reset.trim().parse::<Timestamp>().ok())
        .and_then(|reset| response.until(reset));
    let named = response.retry_after().or(reset);
    response.trouble(waits, named, PLATFORM, || {
        text(&json(response), "error").unwrap_or_default()
    })
}

pub(crate) fn read(response: &Response, waits: Waits) -> Sent {
    if let Some(sent) = trouble(response, waits) {
        return sent;
    }
    let body = json(response);
    text(&body, "id").map_or_else(
        || Sent::Refused("Mastodon's answer holds no id".to_owned()),
        |id| {
            Sent::Landed(Facts {
                message: Some(id),
                thread: None,
                permalink: text(&body, "url"),
            })
        },
    )
}
