use std::collections::BTreeMap;
use std::sync::Mutex;

use herald_ports::{Http, HttpError, Request, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use herald_ports::Segment;

use crate::audit::{credentials, secret_header};
use crate::lock;

/// A request as a fixture holds it: its secrets redacted, a JSON body as JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recorded {
    /// The method.
    pub method: String,
    /// The URL, its secrets redacted.
    pub url: String,
    /// The headers, their secrets redacted.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// The body: JSON where it parses as JSON, otherwise its text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

impl Recorded {
    /// `request` as a fixture holds it.
    #[must_use]
    pub fn of(request: &Request) -> Self {
        Self {
            method: request.method.as_str().to_owned(),
            url: request.url.redacted(),
            headers: request
                .headers
                .iter()
                .map(|(name, value)| (name.clone(), value.redacted()))
                .collect(),
            body: body(&request.body),
        }
    }

    fn scrubbed(self, scrub: &[(String, String)]) -> Self {
        Self {
            url: replaced(&self.url, scrub),
            body: self.body.map(|body| scrubbed(&body, scrub)),
            ..self
        }
    }
}

/// A response as a fixture holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    /// The status code.
    pub status: u16,
    /// The headers.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// The body: JSON where it parses as JSON, otherwise its text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

impl Answer {
    fn of(response: &Response, scrub: &[(String, String)]) -> Self {
        Self {
            status: response.status,
            headers: response.headers.iter().cloned().collect(),
            body: body(&response.body).map(|body| scrubbed(&body, scrub)),
        }
    }

    fn response(&self) -> Response {
        Response {
            status: self.status,
            headers: self
                .headers
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
            body: match &self.body {
                None => Vec::new(),
                Some(Value::String(text)) => text.clone().into_bytes(),
                Some(json) => json.to_string().into_bytes(),
            },
        }
    }
}

/// One request and the response it got.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exchange {
    /// The request.
    pub request: Recorded,
    /// The response.
    pub response: Answer,
}

/// The exchanges of one recorded conversation, in order.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Fixture {
    /// The exchanges.
    pub exchanges: Vec<Exchange>,
}

impl Fixture {
    /// The fixture `json` holds.
    ///
    /// # Errors
    ///
    /// When `json` is not a fixture.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let fixture: Self = serde_json::from_str(json).map_err(|error| error.to_string())?;
        fixture.audit(&[])?;
        Ok(fixture)
    }

    /// Whether the fixture holds no credential: none of `secrets`, nothing
    /// shaped like a token, and no header that only carries credentials
    /// without its secret redacted.
    ///
    /// # Errors
    ///
    /// Each credential found, by where it is and what it looks like, never
    /// by its value.
    pub fn audit(&self, secrets: &[String]) -> Result<(), String> {
        let mut found = Vec::new();
        for (index, exchange) in self.exchanges.iter().enumerate() {
            let text = serde_json::to_string(exchange).unwrap_or_default();
            if secrets
                .iter()
                .any(|secret| !secret.is_empty() && text.contains(secret.as_str()))
            {
                found.push(format!("exchange {index} holds a secret it was handed"));
            }
            found.extend(
                credentials(&text)
                    .into_iter()
                    .map(|what| format!("exchange {index} holds {what}")),
            );
            let headers = exchange
                .request
                .headers
                .iter()
                .chain(&exchange.response.headers);
            found.extend(
                headers
                    .filter(|(name, value)| secret_header(name, value))
                    .map(|(name, _)| format!("exchange {index} has a {name} header in the clear")),
            );
        }
        if found.is_empty() {
            Ok(())
        } else {
            Err(found.join("\n"))
        }
    }

    /// The fixture as pretty JSON, ending in a newline.
    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            "{}\n",
            serde_json::to_string_pretty(&self.exchanges_value()).unwrap_or_default()
        )
    }

    fn exchanges_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

fn body(bytes: &[u8]) -> Option<Value> {
    if bytes.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(bytes);
    Some(serde_json::from_str(&text).unwrap_or_else(|_| Value::String(text.into_owned())))
}

fn replaced(text: &str, scrub: &[(String, String)]) -> String {
    scrub.iter().fold(text.to_owned(), |text, (from, to)| {
        text.replace(from.as_str(), to)
    })
}

fn scrubbed(value: &Value, scrub: &[(String, String)]) -> Value {
    match value {
        Value::String(text) => Value::String(replaced(text, scrub)),
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| scrubbed(item, scrub)).collect())
        }
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(name, field)| (name.clone(), scrubbed(field, scrub)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn secrets(request: &Request) -> Vec<String> {
    let texts = std::iter::once(&request.url).chain(request.headers.iter().map(|(_, value)| value));
    texts
        .flat_map(herald_ports::Text::segments)
        .filter_map(|segment| match segment {
            Segment::Secret(secret) => Some(secret.expose().to_owned()),
            Segment::Plain(_) => None,
        })
        .collect()
}

/// An `Http` that answers from a fixture, in order, and refuses any request
/// the fixture does not hold next.
#[derive(Debug)]
pub struct Replay {
    fixture: Fixture,
    state: Mutex<(usize, Vec<String>)>,
}

impl Replay {
    /// A replay of `fixture`.
    #[must_use]
    pub fn of(fixture: Fixture) -> Self {
        Self {
            fixture,
            state: Mutex::new((0, Vec::new())),
        }
    }

    /// Whether every exchange was asked for, in order, and nothing else was.
    ///
    /// # Errors
    ///
    /// Each request the fixture did not hold, and each exchange never asked for.
    pub fn finish(&self) -> Result<(), String> {
        let (next, misses) = lock(&self.state).clone();
        let unasked: Vec<String> = self
            .fixture
            .exchanges
            .iter()
            .skip(next)
            .map(|exchange| {
                format!(
                    "never asked: {} {}",
                    exchange.request.method, exchange.request.url
                )
            })
            .collect();
        let problems: Vec<String> = misses.into_iter().chain(unasked).collect();
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("\n"))
        }
    }
}

impl Http for Replay {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        let recorded = Recorded::of(&request);
        let mut state = lock(&self.state);
        let expected = self.fixture.exchanges.get(state.0);
        match expected {
            Some(exchange) if exchange.request == recorded => {
                state.0 += 1;
                Ok(exchange.response.response())
            }
            _ => {
                let wanted = expected.map_or_else(
                    || "nothing".to_owned(),
                    |exchange| serde_json::to_string(&exchange.request).unwrap_or_default(),
                );
                let got = serde_json::to_string(&recorded).unwrap_or_default();
                state.1.push(format!("expected {wanted}, got {got}"));
                Err(HttpError::Refused { url: recorded.url })
            }
        }
    }
}

/// An `Http` that sends through another and records each exchange, with its
/// secrets redacted and each scrubbed value replaced. It keeps the value of
/// every secret it sends, and refuses to hand over a recording holding one.
#[derive(Debug)]
pub struct Recorder<H> {
    inner: H,
    scrub: Vec<(String, String)>,
    exchanges: Mutex<Vec<Exchange>>,
    secrets: Mutex<Vec<String>>,
}

impl<H: Http> Recorder<H> {
    /// A recorder sending through `inner`, replacing each `(from, to)` of
    /// `scrub` in what it records.
    #[must_use]
    pub fn new(inner: H, scrub: Vec<(String, String)>) -> Self {
        Self {
            inner,
            scrub,
            exchanges: Mutex::new(Vec::new()),
            secrets: Mutex::new(Vec::new()),
        }
    }

    /// What it recorded, once [`Fixture::audit`] finds no credential in it.
    ///
    /// # Errors
    ///
    /// Each credential the recording holds, so it is never written.
    pub fn fixture(&self) -> Result<Fixture, String> {
        let fixture = Fixture {
            exchanges: lock(&self.exchanges).clone(),
        };
        fixture.audit(&lock(&self.secrets))?;
        Ok(fixture)
    }
}

impl<H: Http> Http for Recorder<H> {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        let recorded = Recorded::of(&request).scrubbed(&self.scrub);
        lock(&self.secrets).extend(secrets(&request));
        let response = self.inner.send(request).await?;
        let exchange = Exchange {
            request: recorded,
            response: Answer::of(&response, &self.scrub),
        };
        lock(&self.exchanges).push(exchange);
        Ok(response)
    }
}

#[cfg(test)]
mod tests;
