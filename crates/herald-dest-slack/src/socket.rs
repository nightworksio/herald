//! Slack's Socket Mode as herald holds it: a connection opened with the
//! app-level token, each envelope acknowledged and each command it carries
//! handed over (ADR-0016). Nothing here does I/O.

use std::collections::BTreeMap;

use herald_core::DestinationName;
use herald_ports::{Method, Request, Response, Secret, Text};
use serde_json::{Value, json};

use crate::interact::{Interacting, Received};

/// What a driver does with one envelope Slack sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enveloped {
    /// The frame that acknowledges it, where it asks for one.
    pub acknowledgement: Option<String>,
    /// The command it carries, where it carries one.
    pub received: Option<Received>,
    /// Whether Slack asked for the connection to be opened again.
    pub reconnect: bool,
}

impl Interacting {
    /// The request that opens a Socket Mode connection with the app-level
    /// token `app`.
    #[must_use]
    pub fn opening(&self, app: &Secret) -> Request {
        Request::new(
            Method::Post,
            Text::plain(format!("{}/apps.connections.open", self.api())),
        )
        .header(
            "Content-Type",
            Text::plain("application/x-www-form-urlencoded"),
        )
        .header("User-Agent", Text::plain(herald_ports::USER_AGENT))
        .header(
            "Authorization",
            Text::plain("Bearer ").then_secret(app.clone()),
        )
    }

    /// What the envelope `frame`, carried to `destination`, asks of a driver.
    #[must_use]
    pub fn envelope(&self, destination: &DestinationName, frame: &str) -> Enveloped {
        let quiet = Enveloped {
            acknowledgement: None,
            received: None,
            reconnect: false,
        };
        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            return quiet;
        };
        let kind = value.get("type").and_then(Value::as_str);
        if kind == Some("disconnect") {
            return Enveloped {
                reconnect: true,
                ..quiet
            };
        }
        let Some(id) = value.get("envelope_id").and_then(Value::as_str) else {
            return quiet;
        };
        let payload = value.get("payload").unwrap_or(&Value::Null);
        let read = match kind {
            Some("slash_commands") => {
                let fields: BTreeMap<String, String> = payload
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_owned())))
                    .collect();
                self.command(destination, &fields).ok()
            }
            Some("interactive") => self.pressed(destination, payload).ok(),
            _ => None,
        };
        let (answer, received) = match read {
            Some(Received::Usage) => (serde_json::from_str::<Value>(&self.usage()).ok(), None),
            Some(Received::Ignored) | None => (None, None),
            Some(received) => (None, Some(received)),
        };
        let acknowledgement = match answer {
            Some(answer) => json!({ "envelope_id": id, "payload": answer }),
            None => json!({ "envelope_id": id }),
        };
        Enveloped {
            acknowledgement: Some(acknowledgement.to_string()),
            received,
            reconnect: false,
        }
    }
}

/// The address a `apps.connections.open` answer opens a connection at.
///
/// # Errors
///
/// What Slack said, where it opened none.
pub fn opened(response: &Response) -> Result<String, String> {
    let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    let url = body
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("wss://"));
    match (body.get("ok").and_then(Value::as_bool), url) {
        (Some(true), Some(url)) => Ok(url.to_owned()),
        _ => Err(format!(
            "Slack opened no Socket Mode connection: {}",
            body.get("error")
                .and_then(Value::as_str)
                .unwrap_or("it answered with no address")
        )),
    }
}

#[cfg(test)]
mod tests;
