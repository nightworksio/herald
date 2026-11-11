use herald_core::{Facts, Sent, Step, Waits};
use herald_ports::{Credential, Response};
use jiff::SignedDuration;
use serde_json::Value;

use crate::discord::{DISCORD, Discord};

/// The id `field` of `value` holds, where it holds one that is not empty.
pub(crate) fn id<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
}

/// The id `field` of `value` holds, as [`id`] reads it, owned.
pub(crate) fn owned(value: &Value, field: &str) -> Option<String> {
    id(value, field).map(ToOwned::to_owned)
}

/// The JSON `response` holds, or null where it holds none.
pub(crate) fn json(response: &Response) -> Value {
    serde_json::from_slice(&response.body).unwrap_or(Value::Null)
}

/// The wait a `429`'s body names, in seconds.
pub(crate) fn waited(body: &Value) -> Option<SignedDuration> {
    body.get("retry_after")
        .and_then(Value::as_f64)
        .and_then(|seconds| SignedDuration::try_from_secs_f64(seconds).ok())
        .filter(|wait| !wait.is_negative())
}

/// What Discord said in `body`: its error's code and message.
pub(crate) fn said(body: &Value) -> String {
    let code = body.get("code").map(Value::to_string).unwrap_or_default();
    let message = body
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    format!("{code} {message}")
}

/// What a failed response means: when to ask again, or why Discord refused.
pub(crate) fn trouble(response: &Response, waits: Waits) -> Option<Sent> {
    let body = json(response);
    response.trouble(waits, waited(&body), DISCORD, || said(&body))
}

pub(crate) fn read(discord: &Discord, step: &Step, response: &Response) -> Sent {
    if let Some(sent) = trouble(response, discord.waits) {
        return sent;
    }
    let body = json(response);
    let webhook = matches!(discord.credential, Credential::Webhook(_));
    let facts = match step {
        Step::Pin => Some(Facts::default()),
        Step::Post if discord.route.forum && webhook => owned(&body, "id").map(|message| Facts {
            message: Some(message),
            thread: owned(&body, "channel_id"),
            permalink: None,
        }),
        Step::Post if discord.route.forum => body
            .get("message")
            .and_then(|message| owned(message, "id"))
            .map(|message| Facts {
                message: Some(message),
                thread: owned(&body, "id"),
                permalink: None,
            }),
        Step::OpenThread => owned(&body, "id").map(|thread| Facts {
            thread: Some(thread),
            ..Facts::default()
        }),
        _ => owned(&body, "id").map(|message| Facts {
            message: Some(message),
            ..Facts::default()
        }),
    };
    facts.map_or_else(
        || Sent::Refused("Discord's answer holds no id".to_owned()),
        Sent::Landed,
    )
}
