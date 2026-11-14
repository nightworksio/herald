//! `herald lambda`: herald as an AWS Lambda function on a custom runtime
//! (ADR-0017). Each invocation is taken from Lambda's runtime API:
//! - a request to the function's URL, or through an HTTP API, is answered
//!   at once, and the work it leaves is handed to the function again as an
//!   event invocation, or done before answering where it cannot be handed;
//! - work handed in so is done over the request, answered again;
//! - a scheduled event polls the sources, keeps house and forgets the keys
//!   read longest ago.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use herald_aws::{
    ACCESS_KEY_ID, Endpoint, REGION, SECRET_ACCESS_KEY, SESSION_TOKEN, Signing, configured, signed,
};
use herald_compose::{Composed, Host as _, Hosting, Left};
use herald_config::{Config, Uses};
use herald_core::media;
use herald_environment::EnvSecrets;
use herald_flows::{Admission, Inbound, Reply};
use herald_ports::{Clock as _, Http, Method, Request, Secret, SecretText, Secrets as _};
use herald_server::Heard;
use jiff::Timestamp;
use serde_json::{Value, json};

use super::output::say;
use super::serving::heard_lines;
use super::stores::Native;
use super::{Connect, Connection, STOPPED};

/// Where Lambda's runtime API is, as Lambda sets it.
const RUNTIME_API: &str = "AWS_LAMBDA_RUNTIME_API";
/// The function's own name, as Lambda sets it.
const FUNCTION: &str = "AWS_LAMBDA_FUNCTION_NAME";
/// The credentials of the function's role, as Lambda sets them.
pub(crate) const CREDENTIALS: [&str; 3] = [ACCESS_KEY_ID, SECRET_ACCESS_KEY, SESSION_TOKEN];
/// The service the function's invocations are signed for.
const SERVICE: &str = "lambda";
/// The version of the runtime API every path names.
const VERSION: &str = "2018-06-01";
/// What an event handing the function its own work says it is.
const WORK: &str = "work";

/// How the function hands itself work: its name, its region, where Lambda
/// is reached, and its role's credentials.
struct Handing {
    function: String,
    region: String,
    endpoint: Endpoint,
    access_key_id: Secret,
    secret_access_key: Secret,
    session_token: Option<Secret>,
}

impl Handing {
    /// How the function hands itself work, where Lambda set all it needs.
    ///
    /// # Errors
    ///
    /// The variable Lambda left unset.
    async fn read(vars: &BTreeMap<String, String>) -> Result<Self, &'static str> {
        let secrets = EnvSecrets::from_vars(vars.clone());
        let [id, key, token] = CREDENTIALS;
        let region = vars.get(REGION).ok_or(REGION)?.clone();
        Ok(Self {
            function: vars.get(FUNCTION).ok_or(FUNCTION)?.clone(),
            endpoint: Endpoint::new(
                SERVICE,
                &region,
                configured(SERVICE, |name| vars.get(name).map(String::as_str)),
            ),
            region,
            access_key_id: secrets.secret(id).await.map_err(|_| id)?,
            secret_access_key: secrets.secret(key).await.map_err(|_| key)?,
            session_token: secrets.secret(token).await.ok(),
        })
    }

    /// The request that invokes the function with `event`, not waiting for
    /// it to finish, signed at `now`.
    fn invocation(&self, event: &Value, now: Timestamp) -> Request {
        let path = format!("/2015-03-31/functions/{}/invocations", self.function);
        let request = Request::new(Method::Post, SecretText::plain(self.endpoint.at(&path)))
            .body(event.to_string().into_bytes());
        signed(
            request,
            &self.endpoint.canonical(&path),
            media::JSON,
            &[("Invocation-Type", "Event")],
            &Signing {
                access_key_id: &self.access_key_id,
                secret_access_key: &self.secret_access_key,
                session_token: self.session_token.as_ref(),
                region: &self.region,
                service: SERVICE,
                host: self.endpoint.host(),
            },
            now,
        )
    }
}

/// A request a function URL or an HTTP API carried, as herald reads it.
struct Carried {
    method: String,
    path: String,
    query: Option<String>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Carried {
    /// The request an HTTP API's event of payload version 2.0 carries, as a
    /// function URL sends it.
    fn of(event: &Value) -> Option<Self> {
        let method = event.pointer("/requestContext/http/method")?.as_str()?;
        let text = |key: &str| event.get(key).and_then(Value::as_str);
        let headers = event
            .get("headers")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_owned())))
            .collect();
        let body = text("body").unwrap_or_default();
        let body = if event.get("isBase64Encoded").and_then(Value::as_bool) == Some(true) {
            STANDARD.decode(body).ok()?
        } else {
            body.as_bytes().to_vec()
        };
        Some(Self {
            method: method.to_owned(),
            path: text("rawPath").unwrap_or("/").to_owned(),
            query: text("rawQueryString")
                .filter(|query| !query.is_empty())
                .map(ToOwned::to_owned),
            headers,
            body,
        })
    }

    /// The request as the event handing it to the function again writes it.
    fn handed(&self) -> Value {
        json!({
            "herald": WORK,
            "requestContext": { "http": { "method": self.method } },
            "rawPath": self.path,
            "rawQueryString": self.query.clone().unwrap_or_default(),
            "headers": self
                .headers
                .iter()
                .map(|(name, value)| (name.clone(), json!(value)))
                .collect::<serde_json::Map<String, Value>>(),
            "body": STANDARD.encode(&self.body),
            "isBase64Encoded": true,
        })
    }

    fn answered<'a>(&'a self, headers: &'a [(&'a str, &'a str)]) -> Inbound<'a> {
        Inbound {
            method: &self.method,
            path: &self.path,
            query: self.query.as_deref(),
            headers,
            body: &self.body,
        }
    }
}

/// What an HTTP API answers for `reply`.
pub(crate) fn replied(reply: &Reply) -> Value {
    let (body, encoded) = match std::str::from_utf8(&reply.body) {
        Ok(text) => (text.to_owned(), false),
        Err(_) => (STANDARD.encode(&reply.body), true),
    };
    let mut headers = serde_json::Map::new();
    headers.insert("content-type".to_owned(), reply.kind.into());
    if let Some(after) = reply.retry_after {
        headers.insert("retry-after".to_owned(), after.to_string().into());
    }
    json!({
        "statusCode": reply.status,
        "headers": headers,
        "body": body,
        "isBase64Encoded": encoded,
    })
}

/// The function: herald composed on the native host, and the runtime API
/// it takes invocations from.
struct Function<'a> {
    composed: Composed<'a, Native<'a>>,
    runtime: Connection,
    api: String,
    handing: Option<(Handing, Connection)>,
    out: RefCell<&'a mut dyn Write>,
}

impl Function<'_> {
    fn heard(&self, heard: Heard) {
        let mut out = self.out.borrow_mut();
        for line in heard_lines(heard) {
            say(&mut **out, &line);
        }
    }

    fn log(&self, line: &str) {
        say(&mut **self.out.borrow_mut(), line);
    }

    /// Every invocation taken from the runtime API and answered, until the
    /// runtime API cannot be reached.
    async fn run(self) -> u8 {
        loop {
            let next = Request::new(
                Method::Get,
                SecretText::plain(format!("{}/{VERSION}/runtime/invocation/next", self.api)),
            );
            let invoked = match self.runtime.send(next).await {
                Ok(invoked) if invoked.succeeded() => invoked,
                Ok(invoked) => {
                    self.log(&format!(
                        "the runtime API answered {}; the function stops",
                        invoked.status
                    ));
                    return STOPPED;
                }
                Err(error) => {
                    self.log(&format!(
                        "the runtime API could not be reached: {error}; the function stops"
                    ));
                    return STOPPED;
                }
            };
            let id = invoked
                .header("Lambda-Runtime-Aws-Request-Id")
                .unwrap_or_default()
                .to_owned();
            let event: Value = serde_json::from_slice(&invoked.body).unwrap_or(Value::Null);
            let (path, body) = match self.invoked(&event).await {
                Ok(answer) => ("response", answer),
                Err(why) => {
                    self.log(&why);
                    (
                        "error",
                        json!({ "errorMessage": why, "errorType": "Unanswered" }),
                    )
                }
            };
            let done = Request::new(
                Method::Post,
                SecretText::plain(format!(
                    "{}/{VERSION}/runtime/invocation/{id}/{path}",
                    self.api
                )),
            )
            .header("Content-Type", SecretText::plain(media::JSON))
            .body(body.to_string().into_bytes());
            if let Err(error) = self.runtime.send(done).await {
                self.log(&format!("the answer to {id} could not be given: {error}"));
            }
        }
    }

    /// What the function answers `event`.
    async fn invoked(&self, event: &Value) -> Result<Value, String> {
        let composed = &self.composed;
        let handed = event.get("herald").and_then(Value::as_str) == Some(WORK);
        if let Some(carried) = Carried::of(event) {
            let headers: Vec<(&str, &str)> = carried
                .headers
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect();
            let inbound = carried.answered(&headers);
            if handed {
                if let Some(left) = composed.answer(&inbound, Admission::Again).await.left {
                    self.heard(Heard::Worked(composed.work(left).await));
                }
                return Ok(json!({}));
            }
            let answered = composed.answer(&inbound, Admission::Once).await;
            if let Some(reason) = answered.refused {
                self.heard(Heard::Refused(reason));
            }
            for line in &answered.said {
                self.log(line);
            }
            if let Some(left) = answered.left {
                self.leave(left, &carried).await;
            }
            return Ok(replied(&answered.reply));
        }
        let scheduled = matches!(
            event.get("source").and_then(Value::as_str),
            Some("aws.events" | "aws.scheduler")
        );
        if !scheduled {
            return Err(
                "herald takes requests, its own work and scheduled events alone".to_owned(),
            );
        }
        self.heard(Heard::Polled(composed.poll().await));
        self.heard(Heard::Kept(composed.housekeep().await));
        Ok(json!({}))
    }

    /// Hands `left`, left by `carried`, to the function again, or does it
    /// now where it cannot be handed.
    async fn leave(&self, left: Left, carried: &Carried) {
        if let Some((handing, http)) = &self.handing {
            let now = self.composed.host().clock().now();
            let invocation = handing.invocation(&carried.handed(), now);
            match http.send(invocation).await {
                Ok(response) if response.succeeded() => return,
                Ok(response) => self.log(&format!(
                    "the work could not be handed on, as Lambda answered {}; it is done now",
                    response.status
                )),
                Err(error) => self.log(&format!(
                    "the work could not be handed on: {error}; it is done now"
                )),
            }
        }
        self.heard(Heard::Worked(self.composed.work(left).await));
    }
}

pub(crate) async fn lambda(
    config: Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'_>,
    out: &mut dyn Write,
) -> Result<u8, String> {
    let api = vars
        .get(RUNTIME_API)
        .filter(|api| !api.is_empty())
        .map(|api| format!("http://{api}"))
        .ok_or_else(|| {
            format!("{RUNTIME_API} is not set; herald lambda runs as a Lambda function's runtime")
        })?;
    let runtime = connect.wait(&api)?;
    let lambda = Hosting::Lambda;
    let native = Native::new(vars, connect);
    let composed = Composed::new(native, config, lambda, Uses::Serves, lambda.name()).await?;
    let (handing, unhanded) = match Handing::read(vars).await {
        Ok(handing) => {
            let http = connect.reach(handing.endpoint.url())?;
            (Some((handing, http)), None)
        }
        Err(unset) => (None, Some(unset)),
    };
    let function = Function {
        composed,
        runtime,
        api,
        handing,
        out: RefCell::new(out),
    };
    if let Some(unset) = unhanded {
        function.log(&format!(
            "{unset} is not set, so the work a request leaves is done before the next request is taken rather than handed to the function again"
        ));
    }
    Ok(function.run().await)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_request_carries_its_query_only_where_it_has_one() {
        let carried = |query: &str| {
            let event = json!({
                "requestContext": { "http": { "method": "GET" } },
                "rawQueryString": query,
            });
            Carried::of(&event).unwrap().query
        };
        assert_eq!(carried("page=2").as_deref(), Some("page=2"));
        assert_eq!(carried(""), None);
    }
}
