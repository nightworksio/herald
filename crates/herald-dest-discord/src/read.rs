use herald_core::{Facts, Sent, Step};
use herald_ports::Response;
use jiff::SignedDuration;
use serde_json::Value;

use crate::discord::{Credential, Discord};

/// How long to wait when Discord is unwell and says nothing of when to ask
/// again.
const OUTAGE: SignedDuration = SignedDuration::from_secs(5);
const TOO_MANY: u16 = 429;
const SERVER: u16 = 500;

fn id(value: &Value) -> Option<String> {
    value
        .get("id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn seconds(seconds: f64) -> SignedDuration {
    SignedDuration::try_from_secs_f64(seconds).unwrap_or(OUTAGE)
}

/// What a failed response means: when to ask again, or why Discord refused.
pub(crate) fn trouble(response: &Response, body: &Value) -> Option<Sent> {
    let after = response
        .header("retry-after")
        .and_then(|after| after.parse::<f64>().ok())
        .map(seconds);
    if response.status == TOO_MANY {
        let wait = body.get("retry_after").and_then(Value::as_f64).map(seconds);
        return Some(Sent::RetryAfter(wait.or(after).unwrap_or(OUTAGE)));
    }
    if response.status >= SERVER {
        return Some(Sent::RetryAfter(after.unwrap_or(OUTAGE)));
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
    if let Some(sent) = trouble(response, &body) {
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
