use std::collections::BTreeMap;

use herald_compose::{Host, KvAt, Reaching, SqlAt};
use jiff::SignedDuration;
use send_wrapper::SendWrapper;
use wasm_bindgen::{JsCast as _, JsValue};

use super::bindings::HostObject;
use super::crypto::{WebSigner, WebVerifier};
use super::ports::{JsClock, JsHttp, JsKv, JsSecrets, JsSql};
use super::{entries, object, settled};
use crate::numbers::millis;

/// A JavaScript host, as herald composes over it.
pub(crate) struct JsHost {
    host: SendWrapper<HostObject>,
    vars: BTreeMap<String, String>,
    secrets: JsSecrets,
}

impl JsHost {
    /// The host `host`, its variables read once.
    pub(crate) fn new(host: HostObject) -> Self {
        let vars = entries(&host.vars()).into_iter().collect();
        let host = SendWrapper::new(host);
        Self {
            secrets: JsSecrets { host: host.clone() },
            host,
            vars,
        }
    }
}

/// `duration` in milliseconds, or undefined where no JavaScript number holds
/// it exactly, which leaves the host's own default.
fn waited(duration: SignedDuration) -> JsValue {
    millis(duration).map_or(JsValue::UNDEFINED, JsValue::from)
}

pub(super) fn sql_at(at: SqlAt<'_>) -> JsValue {
    match at {
        SqlAt::Sqlite { file, busy } => object(&[
            ("kind", "sqlite".into()),
            ("file", file.display().to_string().into()),
            ("busy", waited(busy)),
        ]),
        SqlAt::Postgres {
            url,
            connect,
            statement,
        } => object(&[
            ("kind", "postgres".into()),
            ("url", url.expose().into()),
            ("connect", waited(connect)),
            ("statement", waited(statement)),
        ]),
        SqlAt::D1(binding) => object(&[("kind", "d1".into()), ("binding", binding.into())]),
        SqlAt::DurableObject {
            binding,
            object: name,
        } => object(&[
            ("kind", "durable-object".into()),
            ("binding", binding.into()),
            ("object", name.into()),
        ]),
    }
}

fn kv_at(at: KvAt<'_>) -> JsValue {
    match at {
        KvAt::DenoKv(path) => object(&[
            ("kind", "deno-kv".into()),
            ("path", path.map_or(JsValue::UNDEFINED, JsValue::from_str)),
        ]),
        KvAt::FastlyKv(store) => object(&[("kind", "fastly-kv".into()), ("store", store.into())]),
    }
}

impl Host for JsHost {
    type Http = JsHttp;
    type Secrets = JsSecrets;
    type Clock = JsClock;
    type Signer = WebSigner;
    type Verifier = WebVerifier;
    type Sql = JsSql;
    type Kv = JsKv;

    fn connect(&self, base: &str) -> Result<JsHttp, String> {
        Reaching::of(base).map(|reaching| JsHttp {
            host: self.host.clone(),
            reaching,
        })
    }

    fn var(&self, name: &str) -> Option<&str> {
        self.vars
            .get(name)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
    }

    fn secrets(&self) -> &JsSecrets {
        &self.secrets
    }

    fn clock(&self) -> &JsClock {
        &JsClock
    }

    fn nonce(&self) -> String {
        format!("{:016x}", js_sys::Math::random().to_bits())
    }

    fn signer(&self) -> &WebSigner {
        &WebSigner
    }

    fn verifier(&self) -> &WebVerifier {
        &WebVerifier
    }

    async fn sql(&self, at: SqlAt<'_>) -> Result<JsSql, String> {
        let opened = settled(self.host.sql(&sql_at(at))).await?;
        Ok(JsSql(SendWrapper::new(opened.unchecked_into())))
    }

    async fn kv(&self, at: KvAt<'_>) -> Result<JsKv, String> {
        let opened = settled(self.host.kv(&kv_at(at))).await?;
        Ok(JsKv(SendWrapper::new(opened.unchecked_into())))
    }
}
