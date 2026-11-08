use herald_core::{Facts, Sent, Waits};
use herald_ports::Response;
use jiff::Timestamp;
use serde_json::Value;

pub(crate) fn text(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

/// What a failed response means: when to ask again, or why the server
/// refused.
pub(crate) fn trouble(response: &Response, body: &Value, waits: Waits) -> Option<Sent> {
    let reset = response
        .header("x-ratelimit-reset")
        .and_then(|reset| reset.trim().parse::<Timestamp>().ok())
        .and_then(|reset| response.until(reset));
    let named = response.retry_after().or(reset);
    if let Some(wait) = waits.after(response.status, named) {
        return Some(Sent::RetryAfter(wait));
    }
    if response.succeeded() {
        return None;
    }
    let error = text(body, "error").unwrap_or_default();
    Some(Sent::Refused(
        format!("Mastodon refused it with {}: {error}", response.status)
            .trim_end()
            .trim_end_matches(':')
            .to_owned(),
    ))
}

pub(crate) fn read(response: &Response, waits: Waits) -> Sent {
    let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    if let Some(sent) = trouble(response, &body, waits) {
        return sent;
    }
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
