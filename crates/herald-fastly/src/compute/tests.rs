use herald_compose::{Host as _, KvAt};
use herald_ports::{Http as _, HttpError, Kv as _, Method, PUBLIC, Request, Text, Versionstamp};
use herald_records::{Kept, KvRecords};
use herald_testing::store_conformance;

use super::host::FastlyHost;

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
fn a_kv_store_the_service_is_not_linked_to_is_refused_by_name() {
    let refused = pollster::block_on(FastlyHost::new().kv(KvAt::FastlyKv("elsewhere"))).err();
    assert!(refused.is_some_and(|said| said.contains("elsewhere")));
    assert!(pollster::block_on(FastlyHost::new().kv(KvAt::DenoKv(None))).is_err());
}
