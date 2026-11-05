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
use herald_aws::{Signing, signed};
use herald_config::Config;
use herald_flows::{
    Admission, Connected, Inbound, Job, Keys, Ports, Serving, Webhooks, answer, housekeep, poll,
    work,
};
use herald_native::{EnvSecrets, RingSigner, RingVerifier, SystemClock};
use herald_ports::{
    Clock as _, Http, Keyring, Method, PUBLIC, Request, Secret, Secrets as _, Store as Keeps, Text,
};
use herald_records::MemoryKeyring;
use herald_server::{Capped, Heard};
use jiff::Timestamp;
use serde_json::{Value, json};

use super::output::say;
use super::serving::said;
use super::stores::{Commands, reached};
use super::{Connect, Connection, STOPPED, Served};

/// Where Lambda's runtime API is, as Lambda sets it.
const RUNTIME_API: &str = "AWS_LAMBDA_RUNTIME_API";
/// The function's own name, as Lambda sets it.
const FUNCTION: &str = "AWS_LAMBDA_FUNCTION_NAME";
/// The region the function runs in, as Lambda sets it.
const REGION: &str = "AWS_REGION";
/// The credentials of the function's role, as Lambda sets them.
const CREDENTIALS: [&str; 3] = [
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
];
/// The version of the runtime API every path names.
const VERSION: &str = "2018-06-01";
/// The name the function takes leases under.
const RUN: &str = "lambda";
/// What an event handing the function its own work says it is.
const WORK: &str = "work";
/// What a base asking for a connection with no bound on an answer starts
/// with, before the base itself.
pub(crate) const WAITING: &str = "waiting+";

/// How the function hands itself work: its name, its region, and its
/// role's credentials.
struct Handing {
    function: String,
    region: String,
    host: String,
    access_key_id: Secret,
    secret_access_key: Secret,
    session_token: Option<Secret>,
}

impl Handing {
    /// How the function hands itself work, where Lambda set all it needs.
    async fn read(vars: &BTreeMap<String, String>) -> Option<Self> {
        let secrets = EnvSecrets::from_vars(vars.clone());
        let [id, key, token] = CREDENTIALS;
        let region = vars.get(REGION)?.clone();
        Some(Self {
            function: vars.get(FUNCTION)?.clone(),
            host: format!("lambda.{region}.amazonaws.com"),
            region,
            access_key_id: secrets.secret(id).await.ok()?,
            secret_access_key: secrets.secret(key).await.ok()?,
            session_token: secrets.secret(token).await.ok(),
        })
    }

    /// The request that invokes the function with `event`, not waiting for
    /// it to finish, signed at `now`.
    fn invocation(&self, event: &Value, now: Timestamp) -> Request {
        let path = format!("/2015-03-31/functions/{}/invocations", self.function);
        let request = Request::new(
            Method::Post,
            Text::plain(format!("https://{}{path}", self.host)),
        )
        .body(event.to_string().into_bytes());
        signed(
            request,
            &path,
            "application/json",
            &[("Invocation-Type", "Event")],
            &Signing {
                access_key_id: &self.access_key_id,
                secret_access_key: &self.secret_access_key,
                session_token: self.session_token.as_ref(),
                region: &self.region,
                service: "lambda",
                host: &self.host,
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

/// What an HTTP API answers for `status`, `kind` and `body`.
pub(crate) fn replied(status: u16, kind: &str, body: &[u8]) -> Value {
    let (body, encoded) = match std::str::from_utf8(body) {
        Ok(text) => (text.to_owned(), false),
        Err(_) => (STANDARD.encode(body), true),
    };
    json!({
        "statusCode": status,
        "headers": { "content-type": kind },
        "body": body,
        "isBase64Encoded": encoded,
    })
}

/// The function's work, over whichever store the config names.
struct Function<'a> {
    config: &'a Config,
    connected: &'a Connected<Connection>,
    webhooks: &'a Webhooks,
    public: &'a Capped<Connection>,
    runtime: Connection,
    api: String,
    handing: Option<(Handing, Connection)>,
    out: RefCell<&'a mut dyn Write>,
}

impl Served for Function<'_> {
    type Done = u8;

    async fn over<S: Keeps + Keyring>(self, store: &S) -> u8 {
        if self.config.server.keys_in_store() {
            self.keyed(store, store).await
        } else {
            let memory = MemoryKeyring::new(self.config.server.keys_most);
            self.keyed(store, &memory).await
        }
    }
}

impl Function<'_> {
    fn heard(&self, heard: Heard) {
        let mut out = self.out.borrow_mut();
        for line in said(heard) {
            say(&mut **out, &line);
        }
    }

    /// Every invocation taken from the runtime API and answered, over
    /// `store`, checking activities with the keys in `keyring`, until the
    /// runtime API cannot be reached.
    async fn keyed<S: Keeps, K: Keyring>(self, store: &S, keyring: &K) -> u8 {
        let ports = Ports {
            store,
            clock: &SystemClock,
            signer: &RingSigner,
        };
        let keys = Keys {
            keyring,
            refreshed: self.config.server.keys_refreshed,
        };
        let serving = |admission| Serving {
            config: self.config,
            connected: self.connected,
            ports: &ports,
            verifier: &RingVerifier,
            public: self.public,
            keys: &keys,
            webhooks: self.webhooks,
            admission,
        };
        let (once, again) = (serving(Admission::Once), serving(Admission::Again));
        loop {
            let next = Request::new(
                Method::Get,
                Text::plain(format!("{}/{VERSION}/runtime/invocation/next", self.api)),
            );
            let invoked = match self.runtime.send(next).await {
                Ok(invoked) if invoked.succeeded() => invoked,
                Ok(invoked) => {
                    self.said(&format!(
                        "the runtime API answered {}; the function stops",
                        invoked.status
                    ));
                    return STOPPED;
                }
                Err(error) => {
                    self.said(&format!(
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
            let (path, body) = match self.invoked(&once, &again, &event).await {
                Ok(answer) => ("response", answer),
                Err(why) => {
                    self.said(&why);
                    (
                        "error",
                        json!({ "errorMessage": why, "errorType": "Unanswered" }),
                    )
                }
            };
            let done = Request::new(
                Method::Post,
                Text::plain(format!(
                    "{}/{VERSION}/runtime/invocation/{id}/{path}",
                    self.api
                )),
            )
            .header("Content-Type", Text::plain("application/json"))
            .body(body.to_string().into_bytes());
            if let Err(error) = self.runtime.send(done).await {
                self.said(&format!("the answer to {id} could not be given: {error}"));
            }
        }
    }

    fn said(&self, line: &str) {
        say(&mut **self.out.borrow_mut(), line);
    }

    /// What the function answers `event`.
    async fn invoked<S: Keeps, K: Keyring>(
        &self,
        once: &Serving<
            '_,
            S,
            SystemClock,
            RingSigner,
            RingVerifier,
            Connection,
            Capped<Connection>,
            K,
        >,
        again: &Serving<
            '_,
            S,
            SystemClock,
            RingSigner,
            RingVerifier,
            Connection,
            Capped<Connection>,
            K,
        >,
        event: &Value,
    ) -> Result<Value, String> {
        let handed = event.get("herald").and_then(Value::as_str) == Some(WORK);
        if let Some(carried) = Carried::of(event) {
            let headers: Vec<(&str, &str)> = carried
                .headers
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect();
            let inbound = carried.answered(&headers);
            if handed {
                if let Some(job) = answer(again, &inbound).await.job {
                    self.heard(Heard::Worked(work(again, job, RUN).await));
                }
                return Ok(json!({}));
            }
            let answered = answer(once, &inbound).await;
            if let Some(job) = answered.job {
                self.leave(once, job, &carried).await;
            }
            let reply = answered.reply;
            return Ok(replied(reply.status, reply.kind, &reply.body));
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
        let config = once.config;
        let ports = once.ports;
        self.heard(Heard::Polled(
            poll(config, once.connected, ports, RUN).await,
        ));
        self.heard(Heard::Kept(
            housekeep(once.connected, ports.store, ports.clock).await,
        ));
        let before = ports
            .clock
            .now()
            .checked_sub(once.keys.refreshed)
            .unwrap_or(Timestamp::MIN);
        let _ = once.keys.keyring.forget_before(before).await;
        Ok(json!({}))
    }

    /// Hands `job`, left by `carried`, to the function again, or does it
    /// now where it cannot be handed.
    async fn leave<S: Keeps, K: Keyring>(
        &self,
        serving: &Serving<
            '_,
            S,
            SystemClock,
            RingSigner,
            RingVerifier,
            Connection,
            Capped<Connection>,
            K,
        >,
        job: Job,
        carried: &Carried,
    ) {
        if let Some((handing, http)) = &self.handing {
            let invocation = handing.invocation(&carried.handed(), serving.ports.clock.now());
            match http.send(invocation).await {
                Ok(response) if response.succeeded() => return,
                Ok(response) => self.said(&format!(
                    "the work could not be handed on, as Lambda answered {}; it is done now",
                    response.status
                )),
                Err(error) => self.said(&format!(
                    "the work could not be handed on: {error}; it is done now"
                )),
            }
        }
        self.heard(Heard::Worked(work(serving, job, RUN).await));
    }
}

pub(crate) async fn lambda(
    config: &Config,
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
    let runtime = connect(&format!("{WAITING}{api}"))?;
    let (records, connected) = reached(config, vars, connect, Commands::Answered).await?;
    let secrets = EnvSecrets::from_vars(vars.clone());
    let webhooks = herald_flows::webhooks(config, &secrets)
        .await
        .map_err(|error| error.to_string())?;
    let public = Capped::new(connect(PUBLIC)?, config.server.fetches);
    let handing = match Handing::read(vars).await {
        Some(handing) => {
            let http = connect(&format!("https://{}", handing.host))?;
            Some((handing, http))
        }
        None => None,
    };
    let function = Function {
        config,
        connected: &connected,
        webhooks: &webhooks,
        public: &public,
        runtime,
        api,
        handing,
        out: RefCell::new(out),
    };
    Ok(records.serve(&connected, function).await)
}
