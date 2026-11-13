use herald_core::{Facts, Sent, Waits};
use herald_ports::{Response, unreserved};
use jiff::SignedDuration;
use serde_json::Value;

use crate::matrix::{MATRIX, Matrix};
use crate::request::MATRIX_TO;

/// What is said when the homeserver names no account for the token.
pub(crate) const UNNAMED: &str = "the homeserver did not say which account the token is";

/// The JSON `response` holds, or null where it holds none.
pub(crate) fn json(response: &Response) -> Value {
    serde_json::from_slice(&response.body).unwrap_or(Value::Null)
}

pub(crate) fn text<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

/// What a failed response means: when to ask again, or why the homeserver
/// refused.
pub(crate) fn trouble(response: &Response, waits: Waits) -> Option<Sent> {
    let body = json(response);
    response.trouble(waits, waited(&body), MATRIX, || said(&body))
}

/// What the homeserver said in `body`: its error's code and text.
pub(crate) fn said(body: &Value) -> String {
    [text(body, "errcode"), text(body, "error")]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The wait a `429`'s body names, in milliseconds.
pub(crate) fn waited(body: &Value) -> Option<SignedDuration> {
    body.get("retry_after_ms")
        .and_then(Value::as_u64)
        .and_then(|millis| i64::try_from(millis).ok())
        .map(SignedDuration::from_millis)
}

pub(crate) fn read(matrix: &Matrix, response: &Response) -> Sent {
    if let Some(sent) = trouble(response, matrix.waits) {
        return sent;
    }
    let body = json(response);
    text(&body, "event_id").map_or_else(
        || Sent::Refused("the homeserver's answer holds no event id".to_owned()),
        |event| {
            Sent::Landed(Facts {
                message: Some(event.to_owned()),
                thread: None,
                permalink: Some(format!(
                    "{MATRIX_TO}#/{}/{}",
                    unreserved(&matrix.route.room),
                    unreserved(event)
                )),
            })
        },
    )
}
