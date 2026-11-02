use herald_core::{Facts, Sent};
use herald_ports::Response;
use jiff::SignedDuration;
use serde_json::Value;

use crate::matrix::Matrix;

/// How long to wait when the homeserver is unwell and says nothing of when
/// to ask again.
const OUTAGE: SignedDuration = SignedDuration::from_secs(5);
const TOO_MANY: u16 = 429;
const SERVER: u16 = 500;
/// Where a message's permalink starts, as Matrix clients read it.
const PERMALINK: &str = "https://matrix.to/#/";

pub(crate) fn text<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

/// What a failed response means: when to ask again, or why the homeserver
/// refused.
pub(crate) fn trouble(response: &Response, body: &Value) -> Option<Sent> {
    let after = response
        .header("retry-after")
        .and_then(|after| after.parse::<u32>().ok())
        .map(|seconds| SignedDuration::from_secs(i64::from(seconds)));
    if response.status == TOO_MANY {
        let named = body
            .get("retry_after_ms")
            .and_then(Value::as_u64)
            .and_then(|millis| i64::try_from(millis).ok())
            .map(SignedDuration::from_millis);
        return Some(Sent::RetryAfter(named.or(after).unwrap_or(OUTAGE)));
    }
    if response.status >= SERVER {
        return Some(Sent::RetryAfter(after.unwrap_or(OUTAGE)));
    }
    if response.succeeded() {
        return None;
    }
    let said = [text(body, "errcode"), text(body, "error")]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    Some(Sent::Refused(
        format!("Matrix refused it with {}: {said}", response.status)
            .trim_end()
            .trim_end_matches(':')
            .to_owned(),
    ))
}

pub(crate) fn read(matrix: &Matrix, response: &Response) -> Sent {
    let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    if let Some(sent) = trouble(response, &body) {
        return sent;
    }
    text(&body, "event_id").map_or_else(
        || Sent::Refused("the homeserver's answer holds no event id".to_owned()),
        |event| {
            Sent::Landed(Facts {
                message: Some(event.to_owned()),
                thread: None,
                permalink: Some(format!("{PERMALINK}{}/{event}", matrix.route.room)),
            })
        },
    )
}
