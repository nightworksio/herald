use herald_core::{Facts, Sent, Step, Waits};
use herald_ports::Response;
use jiff::SignedDuration;
use serde_json::Value;

use crate::discord::{Credential, Discord};

fn id(value: &Value) -> Option<String> {
    value
        .get("id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

/// The wait a `429`'s body names, in seconds.
fn named(body: &Value) -> Option<SignedDuration> {
    body.get("retry_after")
        .and_then(Value::as_f64)
        .and_then(|seconds| SignedDuration::try_from_secs_f64(seconds).ok())
        .filter(|wait| !wait.is_negative())
}

/// What a failed response means: when to ask again, or why Discord refused.
pub(crate) fn trouble(response: &Response, body: &Value, waits: Waits) -> Option<Sent> {
    let named = named(body).or_else(|| response.retry_after());
    if let Some(wait) = waits.after(response.status, named) {
        return Some(Sent::RetryAfter(wait));
    }
    if response.succeeded() {
        return None;
    }
    let code = body.get("code").map(Value::to_string).unwrap_or_default();
    let message = body
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(Sent::Refused(
        format!(
            "Discord refused it with {}: {code} {message}",
            response.status
        )
        .trim_end()
        .to_owned(),
    ))
}

pub(crate) fn read(discord: &Discord, step: &Step, response: &Response) -> Sent {
    let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    if let Some(sent) = trouble(response, &body, discord.waits) {
        return sent;
    }
    let webhook = matches!(discord.credential, Credential::Webhook(_));
    let facts = match step {
        Step::Pin => Some(Facts::default()),
        Step::Post if discord.route.forum && webhook => id(&body).map(|message| Facts {
            message: Some(message),
            thread: body
                .get("channel_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            permalink: None,
        }),
        Step::Post if discord.route.forum => {
            body.get("message").and_then(id).map(|message| Facts {
                message: Some(message),
                thread: id(&body),
                permalink: None,
            })
        }
        Step::OpenThread => id(&body).map(|thread| Facts {
            thread: Some(thread),
            ..Facts::default()
        }),
        _ => id(&body).map(|message| Facts {
            message: Some(message),
            ..Facts::default()
        }),
    };
    facts.map_or_else(
        || Sent::Refused("Discord's answer holds no id".to_owned()),
        Sent::Landed,
    )
}
