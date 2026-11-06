use std::fmt::Write as _;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use herald_compose::{Host as _, KvAt, SqlAt};
use herald_ports::{
    Algorithm, Clock as _, CryptoError, Frame, Http as _, HttpError, Method, PUBLIC, Request,
    Secret, SecretError, Secrets as _, Signer as _, Socket as _, Sockets as _, Sql as _, Statement,
    Text, Timer as _, Value, Verifier as _,
};
use herald_records::{Kept, KvRecords};
use herald_testing::store_conformance;
use js_sys::{Array, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;

use super::bindings::{HostObject, SocketsObject};
use super::crypto::{WebSigner, WebVerifier};
use super::herald::Herald;
use super::host::JsHost;
use super::sessions::{JsSockets, JsTaker, JsTimer};
use super::{bytes, member, object, strings, strings_of};

#[wasm_bindgen(inline_js = r#"
export function host(vars) {
  const kept = new Map();
  let stamp = 0;
  const named = (key) => JSON.stringify(key);
  return {
    vars,
    secret(name) {
      if (name === "LATER") return Promise.resolve("later");
      if (name === "NOW") return "now";
      return undefined;
    },
    async fetch(request) {
      if (request.url.includes("down")) throw new Error("down");
      const said = `${request.method} ${request.url} ${request.headers.length} ${request.body.length}`;
      return {
        status: 200,
        headers: [["x-public", String(request.public)], ["x-most", String(request.most)]],
        body: new TextEncoder().encode(said),
      };
    },
    async sql(at) {
      if (at.kind !== "d1") throw new Error(`no ${at.kind} here`);
      return {
        async transaction(statements) {
          return statements.map((statement, index) => ({
            rows: [[...statement.params, 1.5]],
            changed: index,
          }));
        },
      };
    },
    async kv(at) {
      if (at.kind !== "deno-kv") throw new Error(`no ${at.kind} here`);
      return {
        async get(key) {
          const found = kept.get(named(key));
          return found ? { value: found.value, versionstamp: found.stamp } : null;
        },
        async setIf(key, value, expected) {
          const found = kept.get(named(key));
          if ((found?.stamp ?? null) !== expected) return null;
          stamp += 1;
          kept.set(named(key), { key, value, stamp: String(stamp) });
          return String(stamp);
        },
        async deleteIf(key, expected) {
          const found = kept.get(named(key));
          if (!found || found.stamp !== expected) return false;
          kept.delete(named(key));
          return true;
        },
        async list(prefix) {
          return [...kept.values()]
            .filter((found) => prefix.every((part, index) => found.key[index] === part))
            .map((found) => ({ key: found.key, value: found.value }));
        },
      };
    },
  };
}
"#)]
extern "C" {
    fn host(vars: &JsValue) -> HostObject;
}

#[wasm_bindgen(inline_js = r#"
export function sockets() {
  return {
    async open(url) {
      if (url.includes("down")) throw new Error("down");
      const frames = [{ text: `opened ${url}` }, { closed: 4000 }];
      return {
        async send(text) {
          if (text === "refused") throw new Error("refused");
          frames.unshift({ text: `echo ${text}` });
        },
        async receive() {
          const frame = frames.shift();
          if (!frame) throw new Error("gone");
          return frame;
        },
      };
    },
  };
}
"#)]
extern "C" {
    fn sockets() -> SocketsObject;
}

const RSA: &str = include_str!("../../../herald-native/fixtures/rsa-2048-pkcs8.test.hex");
const RSA_SPKI: &str = include_str!("../../../herald-native/fixtures/rsa-2048-spki.test.pem");
/// RFC 8410's example Ed25519 private key, as PKCS#8, and the public key
/// it publishes.
const ED25519: &str = include_str!("../../fixtures/ed25519-rfc8410-pkcs8.test.hex");
const ED25519_SPKI: &str = "MCowBQYDK2VwAyEAGb9ECWmEzf6FQbrBZ9w7lshQhqowtrbLDFw4rXAxZuE=";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

fn unhex(text: &str) -> Vec<u8> {
    let text = text.trim();
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
        .collect()
}

fn pem(der: &[u8]) -> String {
    format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        STANDARD.encode(der)
    )
}

fn hosted() -> JsHost {
    JsHost::new(host(&object(&[
        ("GITHUB_REPOSITORY", "cli/cli".into()),
        ("EMPTY", "".into()),
    ])))
}

#[wasm_bindgen_test]
async fn an_hmac_is_rfc_4231_s_and_verifies_only_for_its_message() {
    let message = b"what do ya want for nothing?";
    let mac = WebSigner
        .sign(Algorithm::HmacSha256, &Secret::new("K", "Jefe"), message)
        .await
        .unwrap();
    assert_eq!(
        hex(&mac),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    assert!(
        WebVerifier
            .verify(Algorithm::HmacSha256, b"Jefe", message, &mac)
            .await
    );
    assert!(
        !WebVerifier
            .verify(Algorithm::HmacSha256, b"Jefe", b"else", &mac)
            .await
    );
    assert_eq!(
        WebSigner
            .public(Algorithm::HmacSha256, &Secret::new("K", "Jefe"))
            .await,
        Err(CryptoError::BadKey {
            name: "K".to_owned()
        })
    );
}

#[wasm_bindgen_test]
async fn an_ed25519_key_publishes_rfc_8410_s_public_key_and_verifies_its_own_signature() {
    let key = Secret::new("ED", pem(&unhex(ED25519)));
    let public = WebSigner.public(Algorithm::Ed25519, &key).await.unwrap();
    assert_eq!(public, STANDARD.decode(ED25519_SPKI).unwrap());
    let signature = WebSigner
        .sign(Algorithm::Ed25519, &key, b"note")
        .await
        .unwrap();
    let raw = &public[12..];
    assert!(
        WebVerifier
            .verify(Algorithm::Ed25519, raw, b"note", &signature)
            .await
    );
    assert!(
        !WebVerifier
            .verify(Algorithm::Ed25519, raw, b"other", &signature)
            .await
    );
    assert!(
        !WebVerifier
            .verify(Algorithm::Ed25519, b"short", b"note", &signature)
            .await
    );
    assert_eq!(
        WebSigner
            .sign(
                Algorithm::Ed25519,
                &Secret::new("BAD", "not a key!"),
                b"note"
            )
            .await,
        Err(CryptoError::BadKey {
            name: "BAD".to_owned()
        })
    );
}

#[wasm_bindgen_test]
async fn an_rsa_key_publishes_its_spki_and_verifies_through_its_pkcs1() {
    let key = Secret::new("RSA", pem(&unhex(RSA)));
    let public = WebSigner.public(Algorithm::RsaSha256, &key).await.unwrap();
    assert_eq!(public, herald_keys::pem(RSA_SPKI).unwrap());
    let signature = WebSigner
        .sign(Algorithm::RsaSha256, &key, b"(request-target): post /inbox")
        .await
        .unwrap();
    let pkcs1 = &public[24..];
    assert!(
        WebVerifier
            .verify(
                Algorithm::RsaSha256,
                pkcs1,
                b"(request-target): post /inbox",
                &signature
            )
            .await
    );
}

#[wasm_bindgen_test]
async fn the_host_gives_its_variables_secrets_and_time() {
    let hosted = hosted();
    assert_eq!(hosted.var("GITHUB_REPOSITORY"), Some("cli/cli"));
    assert_eq!(hosted.var("EMPTY"), None);
    assert_eq!(hosted.var("UNSET"), None);
    let secrets = hosted.secrets();
    assert_eq!(secrets.secret("NOW").await.unwrap().expose(), "now");
    assert_eq!(secrets.secret("LATER").await.unwrap().expose(), "later");
    assert_eq!(
        secrets.secret("UNSET").await,
        Err(SecretError::Missing {
            name: "UNSET".to_owned()
        })
    );
    assert!(hosted.clock().now().as_second() > 1_700_000_000);
}

#[wasm_bindgen_test]
async fn a_connection_fetches_through_the_host_only_what_it_may_reach() {
    let hosted = hosted();
    let http = hosted.connect("https://api.example/v1").unwrap();
    let request = Request::new(Method::Post, Text::plain("https://api.example/v1/x"))
        .header("Accept", Text::plain("*/*"))
        .body(b"hi".to_vec());
    let response = http.send(request).await.unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"POST https://api.example/v1/x 1 2");
    assert_eq!(response.header("x-public"), Some("false"));
    assert_eq!(
        http.send(Request::new(
            Method::Get,
            Text::plain("https://other.example/")
        ))
        .await,
        Err(HttpError::Refused {
            url: "https://other.example/".to_owned()
        })
    );
    assert_eq!(
        http.send(Request::new(
            Method::Get,
            Text::plain("https://api.example/down")
        ))
        .await,
        Err(HttpError::Unreachable {
            reason: "down".to_owned()
        })
    );
    let small = Request::new(Method::Get, Text::plain("https://api.example/v1")).at_most(3);
    assert!(matches!(
        http.send(small).await,
        Err(HttpError::Unreachable { .. })
    ));
    let public = hosted.connect(PUBLIC).unwrap();
    let fetched = public
        .send(Request::new(
            Method::Get,
            Text::plain("https://social.example/users/ana"),
        ))
        .await
        .unwrap();
    assert_eq!(fetched.header("x-public"), Some("true"));
    assert!(hosted.connect("http://api.example").is_err());
}

#[wasm_bindgen_test]
async fn a_database_the_host_offers_runs_statements_and_one_it_does_not_is_refused() {
    let hosted = hosted();
    let sql = hosted.sql(SqlAt::D1("HERALD_DB")).await.unwrap();
    let big = 1_i64 << 60;
    let answered = sql
        .transaction(vec![Statement {
            sql: "SELECT ?1, ?2, ?3, ?4".to_owned(),
            params: vec![
                Value::Null,
                Value::Integer(-5),
                Value::Integer(big),
                Value::Text("t".to_owned()),
            ],
        }])
        .await
        .unwrap();
    assert_eq!(
        answered[0].rows,
        [[
            Value::Null,
            Value::Integer(-5),
            Value::Integer(big),
            Value::Text("t".to_owned()),
            Value::Null,
        ]]
    );
    assert_eq!(answered[0].changed, 0);
    let refused = hosted
        .sql(SqlAt::Sqlite(std::path::Path::new("herald.sqlite")))
        .await;
    assert_eq!(refused.err().as_deref(), Some("no sqlite here"));
    assert!(hosted.kv(KvAt::FastlyKv("herald")).await.is_err());
}

#[wasm_bindgen_test]
async fn a_key_value_store_the_host_offers_keeps_every_record_a_store_keeps() {
    let kv = hosted().kv(KvAt::DenoKv(None)).await.unwrap();
    let store = Kept::new(KvRecords::new(kv, "herald"));
    store_conformance(&store).await.unwrap();
}

#[wasm_bindgen_test]
async fn a_composed_herald_answers_polls_and_keeps_house() {
    let composed = Herald::compose(
        host(&object(&[])),
        "version = 1\n[store]\nkind = \"deno-kv\"\n".to_owned(),
        JsValue::UNDEFINED,
        "deno".to_owned(),
    )
    .await
    .unwrap_or_else(|_| panic!("composed"));
    let request = object(&[
        ("method", "GET".into()),
        ("path", "/healthz".into()),
        ("headers", Array::new().into()),
        ("body", Uint8Array::new_with_length(0).into()),
    ]);
    let answered = JsFuture::from(composed.answer(request, false))
        .await
        .unwrap();
    assert_eq!(member(&answered, "status").as_f64(), Some(200.0));
    assert_eq!(bytes(&member(&answered, "body")), b"herald is up");
    assert!(member(&answered, "work").is_undefined());
    let polled = JsFuture::from(composed.poll()).await.unwrap();
    assert_eq!(strings_of(&polled), Vec::<String>::new());
    let kept = JsFuture::from(composed.housekeep()).await.unwrap();
    assert_eq!(strings_of(&kept), Vec::<String>::new());
    assert_eq!(strings(&["a"]).length(), 1);
    let schedule = composed.schedule();
    assert!(member(&schedule, "poll").is_undefined());
    assert!(super::herald::public("1.1.1.1"));
}

#[wasm_bindgen_test]
async fn a_config_with_mistakes_or_an_unkept_store_is_not_composed() {
    let mistaken = Herald::compose(
        host(&object(&[])),
        "version = 1\n[store]\nkind = \"nowhere\"\n".to_owned(),
        JsValue::UNDEFINED,
        "deno".to_owned(),
    )
    .await;
    assert!(mistaken.is_err());
    let unkept = Herald::compose(
        host(&object(&[])),
        "version = 1\n[store]\nkind = \"sqlite\"\n".to_owned(),
        JsValue::UNDEFINED,
        "deno".to_owned(),
    )
    .await;
    assert!(unkept.is_err());
    let nowhere = Herald::compose(
        host(&object(&[])),
        "version = 1\n[store]\nkind = \"markers\"\n".to_owned(),
        JsValue::UNDEFINED,
        "elsewhere".to_owned(),
    )
    .await;
    assert!(nowhere.is_err());
}

#[wasm_bindgen_test]
async fn a_session_s_socket_carries_text_both_ways_and_says_how_it_closed() {
    let opened = JsSockets {
        opener: send_wrapper::SendWrapper::new(sockets()),
        reach: herald_compose::SocketReach::under(herald_compose::SESSIONS),
    };
    assert!(matches!(
        opened.open("wss://evil.example/?ticket=1").await,
        Err(HttpError::Refused { .. })
    ));
    assert!(matches!(
        opened.open("wss://down.discord.gg").await,
        Err(HttpError::Unreachable { .. })
    ));
    let mut socket = opened.open("wss://gateway.discord.gg/?v=10").await.unwrap();
    assert_eq!(
        socket.receive().await,
        Frame::Text("opened wss://gateway.discord.gg/?v=10".to_owned())
    );
    socket.send(Text::plain("hello")).await.unwrap();
    assert!(socket.send(Text::plain("refused")).await.is_err());
    assert_eq!(socket.receive().await, Frame::Text("echo hello".to_owned()));
    assert_eq!(socket.receive().await, Frame::Closed(Some(4000)));
    assert_eq!(socket.receive().await, Frame::Closed(None));
}

#[wasm_bindgen_test]
async fn work_is_taken_up_to_the_most_at_once_and_done_on_the_event_loop() {
    use herald_flows::Taker as _;

    JsTimer.sleep(std::time::Duration::from_millis(1)).await;
    let composed = Herald::compose(
        host(&object(&[])),
        "version = 1\n[store]\nkind = \"markers\"\n".to_owned(),
        JsValue::UNDEFINED,
        "worker".to_owned(),
    )
    .await
    .unwrap_or_else(|_| panic!("composed"));
    let taker = JsTaker {
        composed: composed.composed(),
        working: std::rc::Rc::default(),
        most: 1,
    };
    assert_eq!(taker.place(), Some(()));
    assert_eq!(taker.place(), None);
    let destination = herald_core::DestinationName::new("chat");
    taker.fill(
        (),
        herald_flows::Job::Interact {
            destination: destination.clone(),
            asked: Vec::new(),
        },
    );
    taker.said(&destination, "heard".to_owned());
    JsTimer.sleep(std::time::Duration::from_millis(5)).await;
    assert_eq!(taker.working.get(), 0);
    let held = JsFuture::from(composed.hold(sockets())).await.unwrap();
    assert!(held.is_undefined());
}
