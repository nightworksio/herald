use std::rc::Rc;

use herald_compose::{Composed, Hosting, told};
use herald_config::load;
use herald_flows::{Admission, Inbound};
use js_sys::{Function, Promise, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use super::bindings::HostObject;
use super::host::JsHost;
use super::{bytes, member, object, pairs, strings};
use crate::numbers::millis;
use crate::reaching::public_address;

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
    composed: Rc<Composed<JsHost>>,
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
        let loaded =
            load(CONFIG, &config, &read).map_err(|errors| JsError::new(&errors.to_string()))?;
        let composed = Composed::new(JsHost::new(host), loaded, hosting.stores(), hosting.name())
            .await
            .map_err(|error| JsError::new(&error))?;
        Ok(Self {
            composed: Rc::new(composed),
        })
    }

    /// The answer to `request`, `{ method, path, query?, headers, body }`,
    /// its delivery admitted again where `again`: `{ status, kind, body,
    /// work? }`, `work` settling to what the work it leaves did, in words.
    pub fn answer(&self, request: JsValue, again: bool) -> Promise {
        let composed = Rc::clone(&self.composed);
        future_to_promise(async move {
            let method = member(&request, "method").as_string().unwrap_or_default();
            let path = member(&request, "path").as_string().unwrap_or_default();
            let query = member(&request, "query")
                .as_string()
                .filter(|query| !query.is_empty());
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
            let work = answered.job.map_or(JsValue::UNDEFINED, |job| {
                let composed = Rc::clone(&composed);
                future_to_promise(async move {
                    Ok(strings(&told::worked(&composed.work(job).await)).into())
                })
                .into()
            });
            let reply = answered.reply;
            Ok(object(&[
                ("status", reply.status.into()),
                ("kind", reply.kind.into()),
                ("body", Uint8Array::from(reply.body.as_slice()).into()),
                ("work", work),
            ]))
        })
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
