use std::future::Future;

use herald_ports::{Algorithm, CryptoError, Sealing, Secret, Signer, Verifier};
use js_sys::{Array, Reflect, Uint8Array};
use send_wrapper::SendWrapper;
use wasm_bindgen::{JsCast as _, JsValue};

use super::bindings::Subtle;
use super::{bytes, member, object, settled};
use crate::keys::{Imported, Jwk, published, signing, verifying};

/// The host's Web Crypto.
fn subtle() -> Result<Subtle, String> {
    let crypto = Reflect::get(&js_sys::global(), &JsValue::from_str("crypto"))
        .ok()
        .filter(JsValue::is_object)
        .ok_or_else(|| "this host has no Web Crypto".to_owned())?;
    Ok(member(&crypto, "subtle").unchecked_into())
}

fn algorithm(imported: &Imported) -> JsValue {
    match imported.hash {
        Some(hash) => object(&[("name", imported.name.into()), ("hash", hash.into())]),
        None => object(&[("name", imported.name.into())]),
    }
}

/// `imported` as a Web Crypto key for `usage`, exportable where `exported`.
async fn key(
    subtle: &Subtle,
    imported: &Imported,
    usage: &str,
    exported: bool,
) -> Result<JsValue, String> {
    let data = Uint8Array::from(imported.data.as_slice());
    let usages = Array::of1(&JsValue::from_str(usage));
    settled(subtle.import_key(
        imported.format,
        &data,
        &algorithm(imported),
        exported,
        &usages,
    ))
    .await
}

async fn signed(algorithm_used: Algorithm, key_held: &Secret, message: &[u8]) -> Option<Vec<u8>> {
    let imported = signing(algorithm_used, key_held)?;
    let subtle = subtle().ok()?;
    let made = key(&subtle, &imported, "sign", false).await.ok()?;
    let message = Uint8Array::from(message);
    let signature = settled(subtle.sign(&algorithm(&imported), &made, &message))
        .await
        .ok()?;
    Some(bytes(&signature))
}

async fn public_half(algorithm_used: Algorithm, key_held: &Secret) -> Option<Vec<u8>> {
    let imported = signing(algorithm_used, key_held)?;
    let subtle = subtle().ok()?;
    let made = key(&subtle, &imported, "sign", true).await.ok()?;
    let jwk = settled(subtle.export_key("jwk", &made)).await.ok()?;
    let read = |name: &str| member(&jwk, name).as_string();
    published(
        algorithm_used,
        &Jwk {
            x: read("x"),
            n: read("n"),
            e: read("e"),
        },
    )
}

/// The length of an AES-GCM nonce, before the sealed bytes.
const NONCE: u32 = 12;

/// The AES-256-GCM key HKDF-SHA256 derives from `key` for `purpose`, for
/// `usage`.
async fn sealing(
    subtle: &Subtle,
    key: &Secret,
    purpose: &str,
    usage: &str,
) -> Result<JsValue, String> {
    let raw = Uint8Array::from(key.expose().as_bytes());
    let base = settled(subtle.import_key(
        "raw",
        &raw,
        &object(&[("name", "HKDF".into())]),
        false,
        &Array::of1(&JsValue::from_str("deriveKey")),
    ))
    .await?;
    let hkdf = object(&[
        ("name", "HKDF".into()),
        ("hash", "SHA-256".into()),
        ("salt", Uint8Array::new_with_length(0).into()),
        ("info", Uint8Array::from(purpose.as_bytes()).into()),
    ]);
    let aes = object(&[("name", "AES-GCM".into()), ("length", 256.into())]);
    settled(subtle.derive_key(
        &hkdf,
        &base,
        &aes,
        false,
        &Array::of1(&JsValue::from_str(usage)),
    ))
    .await
}

/// AES-GCM under the nonce `nonce`, bound to `bound`.
fn gcm(nonce: &Uint8Array, bound: &[u8]) -> JsValue {
    object(&[
        ("name", "AES-GCM".into()),
        ("iv", nonce.into()),
        ("additionalData", Uint8Array::from(bound).into()),
    ])
}

async fn sealed(key: &Secret, (purpose, bound): (&str, &[u8]), plain: &[u8]) -> Option<Vec<u8>> {
    let subtle = subtle().ok()?;
    let made = sealing(&subtle, key, purpose, "encrypt").await.ok()?;
    let crypto = Reflect::get(&js_sys::global(), &JsValue::from_str("crypto")).ok()?;
    let nonce = Uint8Array::new_with_length(NONCE);
    let random: js_sys::Function = member(&crypto, "getRandomValues").dyn_into().ok()?;
    random.call1(&crypto, &nonce).ok()?;
    let held = settled(subtle.encrypt(&gcm(&nonce, bound), &made, &Uint8Array::from(plain)))
        .await
        .ok()?;
    Some([nonce.to_vec(), bytes(&held)].concat())
}

async fn opened(key: &Secret, (purpose, bound): (&str, &[u8]), sealed: &[u8]) -> Option<Vec<u8>> {
    let (nonce, held) = sealed.split_at_checked(usize::try_from(NONCE).ok()?)?;
    let subtle = subtle().ok()?;
    let made = sealing(&subtle, key, purpose, "decrypt").await.ok()?;
    let plain = settled(subtle.decrypt(
        &gcm(&Uint8Array::from(nonce), bound),
        &made,
        &Uint8Array::from(held),
    ))
    .await
    .ok()?;
    Some(bytes(&plain))
}

/// Signs with the host's Web Crypto: HMAC under the secret's bytes, and
/// Ed25519 or RSA under the PKCS#8 private key the secret holds as PEM.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct WebSigner;

/// Verifies with the host's Web Crypto: HMAC under the shared secret,
/// Ed25519 under its 32-byte public key, and RSA under its PKCS#1 public
/// key.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct WebVerifier;

impl Signer for WebSigner {
    fn sign(
        &self,
        algorithm: Algorithm,
        key: &Secret,
        message: &[u8],
    ) -> impl Future<Output = Result<Vec<u8>, CryptoError>> + Send {
        let key = key.clone();
        let message = message.to_vec();
        SendWrapper::new(async move {
            signed(algorithm, &key, &message)
                .await
                .ok_or_else(|| CryptoError::BadKey {
                    name: key.name().to_owned(),
                })
        })
    }

    fn public(
        &self,
        algorithm: Algorithm,
        key: &Secret,
    ) -> impl Future<Output = Result<Vec<u8>, CryptoError>> + Send {
        let key = key.clone();
        SendWrapper::new(async move {
            public_half(algorithm, &key)
                .await
                .ok_or_else(|| CryptoError::BadKey {
                    name: key.name().to_owned(),
                })
        })
    }

    fn seal(
        &self,
        key: &Secret,
        sealing: Sealing<'_>,
        plain: &[u8],
    ) -> impl Future<Output = Result<Vec<u8>, CryptoError>> + Send {
        let (key, purpose, bound, plain) = (
            key.clone(),
            sealing.purpose.to_owned(),
            sealing.bound.to_vec(),
            plain.to_vec(),
        );
        SendWrapper::new(async move {
            sealed(&key, (&purpose, &bound), &plain)
                .await
                .ok_or(CryptoError::Unsealable {
                    name: key.name().to_owned(),
                })
        })
    }

    fn open(
        &self,
        key: &Secret,
        sealing: Sealing<'_>,
        sealed: &[u8],
    ) -> impl Future<Output = Result<Vec<u8>, CryptoError>> + Send {
        let (key, purpose, bound, sealed) = (
            key.clone(),
            sealing.purpose.to_owned(),
            sealing.bound.to_vec(),
            sealed.to_vec(),
        );
        SendWrapper::new(async move {
            opened(&key, (&purpose, &bound), &sealed)
                .await
                .ok_or_else(|| CryptoError::Unsealed {
                    name: key.name().to_owned(),
                })
        })
    }
}

impl Verifier for WebVerifier {
    fn verify(
        &self,
        algorithm_used: Algorithm,
        key_held: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> impl Future<Output = bool> + Send {
        let imported = verifying(algorithm_used, key_held);
        let message = message.to_vec();
        let signature = signature.to_vec();
        SendWrapper::new(async move {
            let Ok(subtle) = subtle() else {
                return false;
            };
            let Ok(made) = key(&subtle, &imported, "verify", false).await else {
                return false;
            };
            let checked = subtle.verify(
                &algorithm(&imported),
                &made,
                &Uint8Array::from(signature.as_slice()),
                &Uint8Array::from(message.as_slice()),
            );
            settled(checked)
                .await
                .ok()
                .and_then(|valid| valid.as_bool())
                == Some(true)
        })
    }
}
