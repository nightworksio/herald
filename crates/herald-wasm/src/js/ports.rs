use std::future::Future;

use herald_ports::{
    Changed, Clock, Http, HttpError, Kv, KvError, Pair, Request, Response, Secret, SecretError,
    Secrets, Sql, SqlError, Stamped, Statement, Value, Versionstamp,
};
use jiff::Timestamp;
use js_sys::{Array, BigInt, Uint8Array};
use send_wrapper::SendWrapper;
use wasm_bindgen::{JsCast as _, JsValue};

use super::bindings::{HostObject, KvObject, SqlObject};
use super::{bytes, member, object, pairs, resolved, settled, strings, strings_of};
use crate::numbers::{count, integer, number};
use crate::reaching::{Reaching, received};

/// `Http` through the host's `fetch`, reaching what one base may.
#[derive(Clone)]
pub(crate) struct JsHttp {
    pub(crate) host: SendWrapper<HostObject>,
    pub(crate) reaching: Reaching,
}

impl Http for JsHttp {
    fn send(&self, request: Request) -> impl Future<Output = Result<Response, HttpError>> + Send {
        let host = self.host.clone();
        let reaching = self.reaching.clone();
        SendWrapper::new(async move {
            let url = reaching.admitted(&request.url)?;
            let headers: Array = request
                .headers
                .iter()
                .map(|(name, value)| {
                    Array::of2(
                        &JsValue::from_str(name),
                        &JsValue::from_str(&value.expose()),
                    )
                })
                .collect();
            let most = u32::try_from(request.most).map_or(f64::from(u32::MAX), f64::from);
            let asked = object(&[
                ("method", request.method.as_str().into()),
                ("url", url.as_str().into()),
                ("headers", headers.into()),
                ("body", Uint8Array::from(request.body.as_slice()).into()),
                ("most", most.into()),
                ("public", reaching.public().into()),
            ]);
            let answered = settled(host.fetch(&asked))
                .await
                .map_err(|reason| HttpError::Unreachable { reason })?;
            let status = member(&answered, "status")
                .as_f64()
                .and_then(count)
                .and_then(|status| u16::try_from(status).ok())
                .ok_or_else(|| HttpError::Unreachable {
                    reason: "the host answered with no status".to_owned(),
                })?;
            let response = Response {
                status,
                headers: pairs(&member(&answered, "headers")),
                body: bytes(&member(&answered, "body")),
            };
            received(&request, response)
        })
    }
}

/// The host's secrets.
pub(crate) struct JsSecrets {
    pub(crate) host: SendWrapper<HostObject>,
}

impl Secrets for JsSecrets {
    fn secret(&self, name: &str) -> impl Future<Output = Result<Secret, SecretError>> + Send {
        let host = self.host.clone();
        let name = name.to_owned();
        SendWrapper::new(async move {
            let missing = || SecretError::Missing { name: name.clone() };
            let value = host.secret(&name).map_err(|_| missing())?;
            let value = resolved(value).await.map_err(|_| missing())?;
            value
                .as_string()
                .filter(|value| !value.is_empty())
                .map(|value| Secret::new(name.clone(), value))
                .ok_or_else(missing)
        })
    }
}

/// The host's clock.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct JsClock;

impl Clock for JsClock {
    fn now(&self) -> Timestamp {
        integer(js_sys::Date::now())
            .and_then(|millis| Timestamp::from_millisecond(millis).ok())
            .unwrap_or(Timestamp::UNIX_EPOCH)
    }
}

/// A database the host offers.
pub(crate) struct JsSql(pub(crate) SendWrapper<SqlObject>);

fn bound(value: &Value) -> JsValue {
    match value {
        Value::Null => JsValue::NULL,
        Value::Integer(whole) => {
            number(*whole).map_or_else(|| BigInt::from(*whole).into(), JsValue::from)
        }
        Value::Text(text) => JsValue::from_str(text),
    }
}

fn read(value: &JsValue) -> Value {
    if let Some(text) = value.as_string() {
        Value::Text(text)
    } else if let Some(whole) = value.as_f64().and_then(integer) {
        Value::Integer(whole)
    } else if let Some(big) = value.dyn_ref::<BigInt>() {
        i64::try_from(big.clone()).map_or(Value::Null, Value::Integer)
    } else {
        Value::Null
    }
}

impl Sql for JsSql {
    fn transaction(
        &self,
        statements: Vec<Statement>,
    ) -> impl Future<Output = Result<Vec<Changed>, SqlError>> + Send {
        let sql = self.0.clone();
        SendWrapper::new(async move {
            let sent: Array = statements
                .iter()
                .map(|statement| {
                    let params: Array = statement.params.iter().map(bound).collect();
                    object(&[
                        ("sql", statement.sql.as_str().into()),
                        ("params", params.into()),
                    ])
                })
                .collect();
            let answered = settled(sql.transaction(&sent))
                .await
                .map_err(|reason| SqlError::Unavailable { reason })?;
            Ok(Array::from(&answered)
                .iter()
                .map(|changed| Changed {
                    rows: Array::from(&member(&changed, "rows"))
                        .iter()
                        .map(|row| Array::from(&row).iter().map(|value| read(&value)).collect())
                        .collect(),
                    changed: member(&changed, "changed")
                        .as_f64()
                        .and_then(count)
                        .unwrap_or_default(),
                })
                .collect())
        })
    }
}

/// A key-value store the host offers.
pub(crate) struct JsKv(pub(crate) SendWrapper<KvObject>);

fn unreached(reason: String) -> KvError {
    KvError::Unavailable { reason }
}

fn stamp(value: &JsValue) -> Option<Versionstamp> {
    value.as_string().map(Versionstamp)
}

impl Kv for JsKv {
    fn get(&self, key: &[String]) -> impl Future<Output = Result<Option<Stamped>, KvError>> + Send {
        let kv = self.0.clone();
        let key = strings(key);
        SendWrapper::new(async move {
            let found = settled(kv.get(&key)).await.map_err(unreached)?;
            Ok(
                stamp(&member(&found, "versionstamp")).map(|versionstamp| Stamped {
                    value: bytes(&member(&found, "value")),
                    versionstamp,
                }),
            )
        })
    }

    fn set_if(
        &self,
        key: &[String],
        value: Vec<u8>,
        expected: Option<Versionstamp>,
    ) -> impl Future<Output = Result<Option<Versionstamp>, KvError>> + Send {
        let kv = self.0.clone();
        let key = strings(key);
        SendWrapper::new(async move {
            let expected = expected.map_or(JsValue::NULL, |stamp| JsValue::from_str(&stamp.0));
            let value = Uint8Array::from(value.as_slice());
            let set = settled(kv.set_if(&key, &value, &expected))
                .await
                .map_err(unreached)?;
            Ok(stamp(&set))
        })
    }

    fn delete_if(
        &self,
        key: &[String],
        expected: Versionstamp,
    ) -> impl Future<Output = Result<bool, KvError>> + Send {
        let kv = self.0.clone();
        let key = strings(key);
        SendWrapper::new(async move {
            let deleted = settled(kv.delete_if(&key, &expected.0))
                .await
                .map_err(unreached)?;
            Ok(deleted.as_bool() == Some(true))
        })
    }

    fn list(&self, prefix: &[String]) -> impl Future<Output = Result<Vec<Pair>, KvError>> + Send {
        let kv = self.0.clone();
        let prefix = strings(prefix);
        SendWrapper::new(async move {
            let listed = settled(kv.list(&prefix)).await.map_err(unreached)?;
            Ok(Array::from(&listed)
                .iter()
                .map(|pair| Pair {
                    key: strings_of(&member(&pair, "key")),
                    value: bytes(&member(&pair, "value")),
                })
                .collect())
        })
    }
}
