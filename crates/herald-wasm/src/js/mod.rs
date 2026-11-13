mod bindings;
#[cfg(feature = "conformance")]
mod conformance;
mod crypto;
mod herald;
mod host;
mod ports;
mod sessions;
#[cfg(test)]
mod tests;

use js_sys::{Array, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::{JsCast as _, JsValue};
use wasm_bindgen_futures::JsFuture;

pub use bindings::HostObject;
pub use herald::Herald;

/// The member `name` of `object`, or `undefined`.
fn member(object: &JsValue, name: &str) -> JsValue {
    Reflect::get(object, &JsValue::from_str(name)).unwrap_or(JsValue::UNDEFINED)
}

/// An object of `members`.
fn object(members: &[(&str, JsValue)]) -> JsValue {
    let made = Object::new();
    for (name, value) in members {
        let _ = Reflect::set(&made, &JsValue::from_str(name), value);
    }
    made.into()
}

/// What a JavaScript error says.
fn error_message(error: &JsValue) -> String {
    error
        .dyn_ref::<js_sys::Error>()
        .map(|error| String::from(error.message()))
        .or_else(|| error.as_string())
        .unwrap_or_else(|| format!("{error:?}"))
}

/// What a call answering a promise settles to, or what it threw or
/// rejected with.
async fn settled(called: Result<Promise, JsValue>) -> Result<JsValue, String> {
    let promise = called.map_err(|error| error_message(&error))?;
    JsFuture::from(promise)
        .await
        .map_err(|error| error_message(&error))
}

/// `value`, awaited where it is a promise.
async fn resolved(value: JsValue) -> Result<JsValue, String> {
    match value.dyn_into::<Promise>() {
        Ok(promise) => JsFuture::from(promise)
            .await
            .map_err(|error| error_message(&error)),
        Err(value) => Ok(value),
    }
}

/// `values` as an array of strings.
fn strings<S: AsRef<str>>(values: &[S]) -> Array {
    values
        .iter()
        .map(|value| JsValue::from_str(value.as_ref()))
        .collect()
}

/// The strings of the array `value`.
fn strings_of(value: &JsValue) -> Vec<String> {
    Array::from(value)
        .iter()
        .filter_map(|value| value.as_string())
        .collect()
}

/// The bytes of a `Uint8Array` or `ArrayBuffer`, or of a string's UTF-8.
fn bytes(value: &JsValue) -> Vec<u8> {
    value
        .as_string()
        .map_or_else(|| Uint8Array::new(value).to_vec(), String::into_bytes)
}

/// The name and value pairs of the array of pairs `value`.
fn pairs(value: &JsValue) -> Vec<(String, String)> {
    Array::from(value)
        .iter()
        .filter_map(|pair| {
            let pair = Array::from(&pair);
            Some((pair.get(0).as_string()?, pair.get(1).as_string()?))
        })
        .collect()
}

/// The string members of the object `value`.
fn entries(value: &JsValue) -> Vec<(String, String)> {
    if !value.is_object() {
        return Vec::new();
    }
    Object::entries(value.unchecked_ref())
        .iter()
        .filter_map(|entry| {
            let entry = Array::from(&entry);
            Some((entry.get(0).as_string()?, entry.get(1).as_string()?))
        })
        .collect()
}
