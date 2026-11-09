use std::rc::Rc;

use herald_compose::{Composed, Hosting, SocketReach, asked_query, public_address, told};
use herald_config::{Server, load};
use herald_flows::{Admission, Inbound};
use js_sys::{Function, Promise, Uint8Array};
use send_wrapper::SendWrapper;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use super::bindings::{HostObject, SocketsObject};
use super::host::JsHost;
use super::sessions::{JsSockets, JsTaker, JsTimer};
use super::{bytes, member, object, pairs, strings};
use crate::numbers::millis;

/// Whether `address` is on the public internet: what a host that resolves
/// names itself checks before it connects on others' behalf.
#[wasm_bindgen(js_name = publicAddress)]
pub fn public(address: &str) -> bool {
    public_address(address)
}

/// The file a config is read as, for the places its mistakes are named.
const CONFIG: &str = "herald.toml";

/// herald composed on a JavaScript host.
#[wasm_bindgen]
pub struct Herald {
    composed: Rc<Composed<'static, JsHost>>,
}

#[cfg(test)]
impl Herald {
    pub(crate) fn composed(&self) -> Rc<Composed<'static, JsHost>> {
        Rc::clone(&self.composed)
    }
}

#[wasm_bindgen]
impl Herald {
    /// herald composed on `host` from the config `config`, each template
    /// file it names read from `files`, an object of texts by name or a
    /// function answering each name with its text, serving as the host
    /// named `hosting`: `worker`, `deno`, `node`, `vercel` or `netlify`.
    ///
    /// # Errors
    ///
    /// Where the config has mistakes, or its store, a secret or a base URL
    /// cannot be reached, saying why.
    pub async fn compose(
        host: HostObject,
        config: String,
        files: JsValue,
        hosting: String,
    ) -> Result<Herald, JsError> {
        let hosting = Hosting::of(&hosting).ok_or_else(|| {
            let named: Vec<&str> = Hosting::ALL.iter().map(|hosting| hosting.name()).collect();
            JsError::new(&format!(
                "{hosting} is no host herald knows; it runs on {}",
                named.join(", ")
            ))
        })?;
        let read = |name: &str| {
            let read = match files.dyn_ref::<Function>() {
                Some(reader) => reader.call1(&JsValue::NULL, &JsValue::from_str(name)).ok(),
                None => Some(member(&files, name)),
            };
            read.and_then(|text| text.as_string())
                .ok_or_else(|| format!("{name} is not among the files herald was given"))
        };
        let loaded = load(CONFIG, &config, &read, &herald_compose::kinds())
            .map_err(|errors| JsError::new(&errors.to_string()))?;
        let composed = Composed::new(JsHost::new(host), loaded, hosting.stores(), hosting.name())
            .await
            .map_err(|error| JsError::new(&error))?;
        Ok(Self {
            composed: Rc::new(composed),
        })
    }

    /// The answer to `request`, `{ method, path, query?, headers, body }`,
    /// its delivery admitted again where `again`: `{ status, kind, body,
    /// retryAfter?, work?, refused?, said }`, `work` settling to what the
    /// work it leaves did, in words, and `said` what a round it asked for
    /// did.
    pub fn answer(&self, request: JsValue, again: bool) -> Promise {
        let composed = Rc::clone(&self.composed);
        future_to_promise(async move {
            let method = member(&request, "method").as_string().unwrap_or_default();
            let path = member(&request, "path").as_string().unwrap_or_default();
            let query = asked_query(member(&request, "query").as_string());
            let headers = pairs(&member(&request, "headers"));
            let headers: Vec<(&str, &str)> = headers
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect();
            let body = bytes(&member(&request, "body"));
            let inbound = Inbound {
                method: &method,
                path: &path,
                query: query.as_deref(),
                headers: &headers,
                body: &body,
            };
            let admission = if again {
                Admission::Again
            } else {
                Admission::Once
            };
            let answered = composed.answer(&inbound, admission).await;
            let work = answered.left.map_or(JsValue::UNDEFINED, |left| {
                let composed = Rc::clone(&composed);
                future_to_promise(async move {
                    Ok(strings(&told::worked(&composed.work(left).await)).into())
                })
                .into()
            });
            let reply = answered.reply;
            let refused = answered
                .refused
                .as_deref()
                .map_or(JsValue::UNDEFINED, |reason| told::refused(reason).into());
            Ok(object(&[
                ("status", reply.status.into()),
                ("kind", reply.kind.into()),
                ("body", Uint8Array::from(reply.body.as_slice()).into()),
                (
                    "retryAfter",
                    reply.retry_after.map_or(JsValue::UNDEFINED, JsValue::from),
                ),
                ("work", work),
                ("refused", refused),
                ("said", strings(&answered.said).into()),
            ]))
        })
    }

    /// The limits a host that serves herald keeps: `{ body, connections,
    /// read, held }`, the most bytes of a request's body that are read, how
    /// many connections are held at once, how many milliseconds a request's
    /// headers and then its body may take, and how many a connection is held
    /// at most (ADR-0014).
    #[must_use]
    pub fn limits(&self) -> JsValue {
        let server = &self.composed.config().server;
        let period = |every| millis(every).map_or(JsValue::UNDEFINED, JsValue::from);
        object(&[
            ("body", JsValue::from(server.body)),
            ("connections", JsValue::from(server.connections)),
            ("read", period(Server::READ)),
            ("held", period(Server::HELD)),
        ])
    }

    /// Where a host that serves herald listens: `{ hostname, port }` from
    /// `given`, the host's `HERALD_LISTEN`, where it is set, and otherwise
    /// from the config's `[server] listen`. An IPv6 hostname is unbracketed.
    ///
    /// # Errors
    ///
    /// `given` names no address and port.
    pub fn listen(&self, given: Option<String>) -> Result<JsValue, JsError> {
        let named = given.unwrap_or_else(|| self.composed.config().server.listen.clone());
        let address: std::net::SocketAddr = named.parse().map_err(|_| {
            JsError::new(&format!(
                "HERALD_LISTEN is {named}, which names no address and port"
            ))
        })?;
        Ok(object(&[
            ("hostname", JsValue::from(address.ip().to_string())),
            ("port", JsValue::from(address.port())),
        ]))
    }

    /// The periods the config asks rounds at, in milliseconds: `{ poll?,
    /// housekeep? }`.
    #[must_use]
    pub fn schedule(&self) -> JsValue {
        let schedule = self.composed.config().schedule;
        let period = |every: Option<jiff::SignedDuration>| {
            every
                .and_then(millis)
                .map_or(JsValue::UNDEFINED, JsValue::from)
        };
        object(&[
            ("poll", period(schedule.poll)),
            ("housekeep", period(schedule.housekeep)),
        ])
    }

    /// Every session the config's destinations hold, opened through
    /// `sockets`, each command's work done here; settles once every session
    /// has stopped.
    pub fn hold(&self, sockets: SocketsObject) -> Promise {
        let composed = Rc::clone(&self.composed);
        future_to_promise(async move {
            let sockets = JsSockets {
                opener: SendWrapper::new(sockets),
                reach: SocketReach::under(composed.config().session_domains()),
            };
            let taker = JsTaker {
                composed: Rc::clone(&composed),
            };
            composed.hold(&sockets, &taker, &JsTimer).await;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Every source polled, settling to what each poll did, in words.
    pub fn poll(&self) -> Promise {
        let composed = Rc::clone(&self.composed);
        future_to_promise(async move {
            let swept = composed.poll().await;
            let said: Vec<String> = swept.iter().flat_map(told::polled).collect();
            Ok(strings(&said).into())
        })
    }

    /// The store's housekeeping done, and the keys read longest ago
    /// forgotten, settling to what it did, in words.
    pub fn housekeep(&self) -> Promise {
        let composed = Rc::clone(&self.composed);
        future_to_promise(
            async move { Ok(strings(&told::kept(&composed.housekeep().await)).into()) },
        )
    }
}
