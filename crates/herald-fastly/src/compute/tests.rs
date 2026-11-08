use std::collections::BTreeMap;

use fastly::Backend;
use herald_compose::{Host as _, KvAt};
use herald_ports::{
    Http as _, HttpError, Kv as _, Method, PUBLIC, Request, SecretError, Secrets as _, Text,
    Versionstamp,
};
use herald_records::{Kept, KvRecords};
use herald_testing::store_conformance;

use super::host::{FastlyHost, StoreSecrets};
use super::http::backend_name;

#[test]
fn every_origin_has_its_own_backend_name() {
    let names = [
        backend_name("https", "hooks.slack.com", 443),
        backend_name("https", "hooks-slack.com", 443),
        backend_name("http", "hooks.slack.com", 443),
        backend_name("https", "hooks.slack.com", 8443),
    ];
    for (index, name) in names.iter().enumerate() {
        assert!(name.starts_with("h_") && name.len() == 66, "{name}");
        assert!(!names[index + 1..].contains(name), "{name}");
    }
    assert_eq!(names[0], backend_name("https", "hooks.slack.com", 443));
}

#[test]
fn the_service_reads_its_config_store_and_secret_store() {
    let host = FastlyHost::new();
    assert!(host.item("herald.toml").unwrap().contains("fastly-kv"));
    assert_eq!(host.item("ABSENT"), None);
    let held = pollster::block_on(host.secrets().secret("HERALD_ROUND_SECRET")).unwrap();
    assert_eq!(held.expose(), "local-round-secret");
    assert!(pollster::block_on(host.secrets().secret("ABSENT")).is_err());
    assert!(matches!(
        pollster::block_on(StoreSecrets.secret("EMPTY")),
        Err(SecretError::Missing { .. })
    ));
}

#[test]
fn the_service_gives_the_variables_it_holds_and_not_an_empty_one() {
    let host = FastlyHost {
        config: None,
        vars: BTreeMap::from([
            ("GITHUB_REPOSITORY", "cli/cli".to_owned()),
            ("EMPTY", String::new()),
        ]),
        secrets: StoreSecrets,
    };
    assert_eq!(host.var("GITHUB_REPOSITORY"), Some("cli/cli"));
    assert_eq!(host.var("EMPTY"), None);
    assert_eq!(host.var("UNSET"), None);
}

#[test]
fn each_nonce_the_service_gives_is_new() {
    let host = FastlyHost {
        config: None,
        vars: BTreeMap::new(),
        secrets: StoreSecrets,
    };
    assert_ne!(host.nonce(), host.nonce());
}

#[test]
fn a_connection_fetches_plain_http_on_loopback_without_tls() {
    const BASE: &str = "http://127.0.0.1:8099";
    let http = FastlyHost::new().connect(BASE).unwrap();
    let mut asked = Request::new(Method::Post, Text::plain(format!("{BASE}/said")));
    asked.body = b"hello".to_vec();
    let answer = pollster::block_on(http.send(asked)).unwrap();
    assert_eq!(answer.status, 200);
    assert_eq!(answer.body, b"POST /said hello");
}

#[test]
fn a_connection_uses_the_backend_its_origin_already_has() {
    const BASE: &str = "http://127.0.0.1:8099";
    let http = FastlyHost::new().connect(BASE).unwrap();
    for path in ["first", "again"] {
        let asked = Request::new(Method::Get, Text::plain(format!("{BASE}/{path}")));
        let answer = pollster::block_on(http.send(asked)).unwrap();
        assert_eq!(answer.body, format!("GET /{path} ").into_bytes());
    }
}

#[test]
fn a_connection_refuses_a_backend_under_its_origin_s_name_that_reaches_elsewhere() {
    let elsewhere = [
        ("http", 8091, "127.0.0.2:8091"),
        ("http", 8092, "127.0.0.1:8093"),
        ("https", 8094, "127.0.0.1:8094"),
    ];
    for (scheme, port, target) in elsewhere {
        Backend::builder(backend_name(scheme, "127.0.0.1", port), target)
            .finish()
            .unwrap();
        let origin = format!("{scheme}://127.0.0.1:{port}");
        let http = FastlyHost::new().connect(&origin).unwrap();
        let asked = Request::new(Method::Get, Text::plain(format!("{origin}/")));
        assert!(
            matches!(
                pollster::block_on(http.send(asked)),
                Err(HttpError::Unreachable { reason }) if reason.contains("another origin")
            ),
            "{origin}"
        );
    }
}

#[test]
fn a_connection_refuses_what_it_may_not_reach() {
    let host = FastlyHost::new();
    let http = host.connect("https://discord.com/api").unwrap();
    let elsewhere = Request::new(Method::Get, Text::plain("https://example.org/"));
    assert!(pollster::block_on(http.send(elsewhere)).is_err());
    assert!(host.connect("not a url").is_err());
}

#[test]
fn a_public_connection_refuses_a_name_on_a_service_not_said_to_be_guarded() {
    let http = FastlyHost::new().connect(PUBLIC).unwrap();
    let asked = Request::new(Method::Get, Text::plain("https://localhost:9/"));
    assert!(matches!(
        pollster::block_on(http.send(asked)),
        Err(HttpError::Refused { .. })
    ));
}

fn kv() -> super::kv::FastlyKv {
    pollster::block_on(FastlyHost::new().kv(KvAt::FastlyKv("herald"))).unwrap()
}

#[test]
fn a_kv_store_the_service_is_linked_to_keeps_every_record_a_store_keeps() {
    let store = Kept::new(KvRecords::new(kv(), "conformance"));
    pollster::block_on(store_conformance(&store)).unwrap();
}

#[test]
fn a_kv_store_writes_and_deletes_only_at_the_generation_it_was_given() {
    let kv = kv();
    let key = ["held".to_owned(), "one".to_owned()];
    pollster::block_on(async {
        let stamp = kv
            .set_if(&key, b"value".to_vec(), None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(kv.set_if(&key, b"again".to_vec(), None).await, Ok(None));
        let stale = Versionstamp("1".to_owned());
        assert_eq!(kv.delete_if(&key, stale).await, Ok(false));
        let found = kv.get(&key).await.unwrap().unwrap();
        assert_eq!(found.value, b"value");
        let listed = kv.list(&["held".to_owned()]).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key, key);
        assert_eq!(kv.delete_if(&key, stamp).await, Ok(true));
        assert_eq!(kv.get(&key).await, Ok(None));
    });
}

#[test]
fn a_deleted_record_is_absent_and_written_again_once() {
    let kv = kv();
    let key = ["held".to_owned(), "deleted".to_owned()];
    pollster::block_on(async {
        let first = kv.set_if(&key, b"first".to_vec(), None).await.unwrap();
        assert_eq!(kv.delete_if(&key, first.unwrap()).await, Ok(true));
        assert_eq!(kv.get(&key).await, Ok(None));
        assert_eq!(kv.list(&["held".to_owned()]).await, Ok(Vec::new()));
        let again = kv.set_if(&key, b"again".to_vec(), None).await.unwrap();
        assert!(again.is_some());
        assert_eq!(kv.get(&key).await.unwrap().unwrap().value, b"again");
        assert_eq!(kv.set_if(&key, b"twice".to_vec(), None).await, Ok(None));
        assert_eq!(kv.delete_if(&key, again.unwrap()).await, Ok(true));
    });
}

#[test]
fn a_kv_store_the_service_is_not_linked_to_is_refused_by_name() {
    let refused = pollster::block_on(FastlyHost::new().kv(KvAt::FastlyKv("elsewhere"))).err();
    assert!(refused.is_some_and(|said| said.contains("elsewhere")));
    assert!(pollster::block_on(FastlyHost::new().kv(KvAt::DenoKv(None))).is_err());
}
