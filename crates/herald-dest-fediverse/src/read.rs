use herald_core::{Facts, Sent};
use herald_ports::Response;
use jiff::SignedDuration;
use serde_json::Value;

/// How long to wait when the server is unwell and says nothing of when to
/// ask again.
const OUTAGE: SignedDuration = SignedDuration::from_secs(5);
/// How long to wait on a `429` that names no wait: Mastodon's limits are
/// counted in windows of five minutes.
const LIMITED: SignedDuration = SignedDuration::from_mins(5);
const TOO_MANY: u16 = 429;
const SERVER: u16 = 500;

pub(crate) fn text(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

/// What a failed response means: when to ask again, or why the server
/// refused.
pub(crate) fn trouble(response: &Response, body: &Value) -> Option<Sent> {
    let after = response
        .header("retry-after")
        .and_then(|after| after.parse::<u32>().ok())
        .map(|seconds| SignedDuration::from_secs(i64::from(seconds)));
    if response.status == TOO_MANY {
        return Some(Sent::RetryAfter(after.unwrap_or(LIMITED)));
    }
    if response.status >= SERVER {
        return Some(Sent::RetryAfter(after.unwrap_or(OUTAGE)));
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

pub(crate) fn read(response: &Response) -> Sent {
    let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    if let Some(sent) = trouble(response, &body) {
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
