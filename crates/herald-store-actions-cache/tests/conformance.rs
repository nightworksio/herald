//! The Actions-cache store against a model of the cache service, through the
//! conformance suite every store passes, and through the races and refusals
//! the service answers with.

#[cfg(test)]
#[allow(
    clippy::unused_async_trait_impl,
    reason = "the model answers at once, and is written as the async trait it fakes"
)]
mod conformance {
    use std::sync::{Arc, Mutex};

    use herald_core::{Entry, Identity, Key, RouteId, Subscription};
    use herald_ports::{
        Http, HttpError, Method, Request, Response, Secret, Store, StoreError, Version, Written,
    };
    use herald_store_actions_cache::{ActionsCache, ActionsCacheStore, Blobs};
    use herald_testing::store_conformance;
    use serde_json::{Value, json};

    const RESULTS: &str = "https://results.example/";
    const BLOBS: &str = "https://blobs.example";
    const SERVICE: &str =
        "https://results.example/twirp/github.actions.results.api.v1.CacheService/";

    #[derive(Debug, Default)]
    struct State {
        entries: Vec<(String, Option<Vec<u8>>)>,
        uploads: Vec<(String, Vec<u8>)>,
        fail: Option<(String, u16, String)>,
        upload_status: Option<u16>,
        blob_origin: Option<String>,
    }

    #[derive(Debug, Clone, Default)]
    struct Cache {
        state: Arc<Mutex<State>>,
    }

    fn answer(status: u16, body: &Value) -> Response {
        Response {
            status,
            headers: Vec::new(),
            body: body.to_string().into_bytes(),
        }
    }

    impl State {
        fn twirp(&mut self, method: &str, body: &Value) -> Response {
            if let Some((failing, status, answered)) = &self.fail
                && failing == method
            {
                return Response {
                    status: *status,
                    headers: Vec::new(),
                    body: answered.clone().into_bytes(),
                };
            }
            assert_eq!(
                body["version"],
                "514567f3aac3eee4eded2f7416de16186bc488fbfc99341324de7a200c712123"
            );
            let key = body["key"].as_str().unwrap_or_default().to_owned();
            let origin = self.blob_origin.clone().unwrap_or_else(|| BLOBS.to_owned());
            match method {
                "GetCacheEntryDownloadURL" => {
                    let prefix = body["restore_keys"][0].as_str().unwrap_or_default();
                    let found = self
                        .entries
                        .iter()
                        .rev()
                        .find(|(key, data)| key.starts_with(prefix) && data.is_some());
                    match found {
                        Some((key, _)) => answer(
                            200,
                            &json!({
                                "ok": true,
                                "signedDownloadUrl": format!("{origin}/{key}?sig=read"),
                                "matchedKey": key,
                            }),
                        ),
                        None => answer(200, &json!({ "ok": false })),
                    }
                }
                "CreateCacheEntry" => {
                    if self.entries.iter().any(|(held, _)| *held == key) {
                        return answer(409, &json!({ "code": "already_exists", "msg": "exists" }));
                    }
                    self.entries.push((key.clone(), None));
                    answer(
                        200,
                        &json!({ "ok": true, "signed_upload_url": format!("{origin}/{key}?sig=write") }),
                    )
                }
                "FinalizeCacheEntryUpload" => {
                    let Some(at) = self.uploads.iter().position(|(held, _)| *held == key) else {
                        return answer(200, &json!({ "ok": false }));
                    };
                    let (_, bytes) = self.uploads.remove(at);
                    assert_eq!(body["size_bytes"], bytes.len().to_string());
                    for entry in &mut self.entries {
                        if entry.0 == key {
                            entry.1 = Some(bytes.clone());
                        }
                    }
                    answer(200, &json!({ "ok": true, "entry_id": "1" }))
                }
                _ => answer(404, &json!({})),
            }
        }

        fn blob(&mut self, method: Method, url: &str, body: Vec<u8>) -> Response {
            let path = url
                .split_once("://")
                .and_then(|(_, rest)| rest.split_once('/'))
                .map(|(_, path)| path.to_owned())
                .unwrap_or_default();
            let key = path.split('?').next().unwrap_or_default().to_owned();
            if method == Method::Put {
                assert!(path.ends_with("?sig=write"));
                self.uploads.push((key, body));
                return Response {
                    status: self.upload_status.unwrap_or(201),
                    headers: Vec::new(),
                    body: Vec::new(),
                };
            }
            let data = self
                .entries
                .iter()
                .find(|(held, _)| *held == key)
                .and_then(|(_, data)| data.clone())
                .unwrap_or_default();
            Response {
                status: 200,
                headers: Vec::new(),
                body: data,
            }
        }
    }

    impl Http for Cache {
        async fn send(&self, request: Request) -> Result<Response, HttpError> {
            let mut state = self.state.lock().unwrap();
            let url = request.url.expose();
            if let Some(method) = url.strip_prefix(SERVICE) {
                assert_eq!(
                    request
                        .headers
                        .iter()
                        .find(|(name, _)| name == "Authorization")
                        .map(|(_, value)| value.redacted()),
                    Some("Bearer «secret:ACTIONS_RUNTIME_TOKEN»".to_owned())
                );
                let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
                return Ok(state.twirp(method, &body));
            }
            assert_eq!(
                request.url.redacted(),
                "«secret:the Actions cache's signed URL»"
            );
            Ok(state.blob(request.method, &url, request.body))
        }
    }

    fn blobs(cache: &Cache) -> Blobs<'static, Cache> {
        let cache = cache.clone();
        Box::new(move |origin| {
            if origin == BLOBS {
                Ok(cache.clone())
            } else {
                Err(format!("{origin} cannot be reached"))
            }
        })
    }

    fn store(cache: &Cache) -> ActionsCacheStore<'static, Cache> {
        ActionsCache::new(RESULTS, Secret::new("ACTIONS_RUNTIME_TOKEN", "t"), None)
            .store(cache.clone(), blobs(cache))
    }

    fn key(identity: &str) -> Key {
        Key::derive(&RouteId::new("r"), &Identity::new(identity))
    }

    #[test]
    fn the_store_passes_the_conformance_suite() {
        let cache = Cache::default();
        assert_eq!(
            pollster::block_on(store_conformance(&store(&cache))),
            Ok(())
        );
    }

    #[test]
    fn each_write_saves_the_next_generation_which_a_later_run_reads() {
        let cache = Cache::default();
        let first = store(&cache);
        let entry = Entry::new(key("a"));
        pollster::block_on(first.put_entry(&entry, None)).unwrap();
        pollster::block_on(first.put_entry(&entry, Some(Version(1)))).unwrap();
        let keys: Vec<String> = cache
            .state
            .lock()
            .unwrap()
            .entries
            .iter()
            .map(|(key, _)| key.clone())
            .collect();
        assert_eq!(
            keys,
            [
                "herald-state-00000000000000000001",
                "herald-state-00000000000000000002"
            ]
        );
        let later = store(&cache);
        let read = pollster::block_on(later.entry(&key("a"))).unwrap().unwrap();
        assert_eq!((read.value, read.version), (entry, Version(2)));
    }

    #[test]
    fn a_run_that_loses_a_generation_to_another_reads_it_and_loses_nothing() {
        let cache = Cache::default();
        let one = store(&cache);
        let other = store(&cache);
        assert_eq!(pollster::block_on(one.entry(&key("b"))), Ok(None));
        pollster::block_on(other.put_entry(&Entry::new(key("a")), None)).unwrap();
        assert_eq!(
            pollster::block_on(one.put_entry(&Entry::new(key("b")), None)),
            Ok(Written::Written(Version(1)))
        );
        assert_eq!(
            pollster::block_on(one.put_entry(&Entry::new(key("a")), None)),
            Ok(Written::Conflict)
        );
        let later = store(&cache);
        for identity in ["a", "b"] {
            assert!(
                pollster::block_on(later.entry(&key(identity)))
                    .unwrap()
                    .is_some()
            );
        }
        let subscription = Subscription {
            destination: herald_core::DestinationName::new("slack"),
            project: "p".to_owned(),
            subscriber: "u".to_owned(),
        };
        pollster::block_on(one.subscribe(&subscription)).unwrap();
        pollster::block_on(other.unsubscribe(&subscription)).unwrap();
        assert_eq!(pollster::block_on(one.unsubscribe(&subscription)), Ok(()));
    }

    fn failing(method: &str, status: u16, body: &str) -> Cache {
        let cache = Cache::default();
        cache.state.lock().unwrap().fail = Some((method.to_owned(), status, body.to_owned()));
        cache
    }

    fn unavailable(reason: &str) -> StoreError {
        StoreError::Unavailable {
            reason: reason.to_owned(),
        }
    }

    #[test]
    fn what_the_cache_service_refuses_or_garbles_says_so() {
        let entry = Entry::new(key("a"));
        assert_eq!(
            pollster::block_on(
                store(&failing("GetCacheEntryDownloadURL", 500, "{}")).entry(&key("a"))
            ),
            Err(unavailable("the Actions cache answered 500"))
        );
        let garbled = pollster::block_on(
            store(&failing("GetCacheEntryDownloadURL", 200, "nonsense")).entry(&key("a")),
        );
        assert!(
            matches!(garbled, Err(StoreError::Corrupt { .. })),
            "{garbled:?}"
        );
        let stranger = failing(
            "GetCacheEntryDownloadURL",
            200,
            r#"{"ok":true,"signedDownloadUrl":"https://blobs.example/x","matchedKey":"someone-else-1"}"#,
        );
        let error = pollster::block_on(store(&stranger).entry(&key("a"))).unwrap_err();
        assert!(error.to_string().contains("someone-else-1"), "{error}");
        let plain = failing(
            "GetCacheEntryDownloadURL",
            200,
            r#"{"ok":true,"signedDownloadUrl":"http://blobs.example/x","matchedKey":"herald-state-1"}"#,
        );
        assert_eq!(
            pollster::block_on(store(&plain).entry(&key("a"))),
            Err(unavailable(
                "the Actions cache named a blob URL that is not HTTPS"
            ))
        );
        let elsewhere = Cache::default();
        elsewhere.state.lock().unwrap().blob_origin = Some("https://other.example".to_owned());
        assert_eq!(
            pollster::block_on(store(&elsewhere).put_entry(&entry, None)),
            Err(unavailable("https://other.example cannot be reached"))
        );
        assert_eq!(
            pollster::block_on(
                store(&failing("CreateCacheEntry", 200, r#"{"ok":false}"#)).put_entry(&entry, None)
            ),
            Ok(Written::Conflict)
        );
        let refused = Cache::default();
        refused.state.lock().unwrap().upload_status = Some(403);
        assert_eq!(
            pollster::block_on(store(&refused).put_entry(&entry, None)),
            Err(unavailable("the Actions cache answered 403"))
        );
        assert_eq!(
            pollster::block_on(
                store(&failing("FinalizeCacheEntryUpload", 200, r#"{"ok":false}"#))
                    .put_entry(&entry, None)
            ),
            Err(unavailable(
                "the Actions cache did not keep herald-state-00000000000000000001"
            ))
        );
    }

    #[test]
    fn a_generation_that_cannot_be_read_says_which() {
        let cache = Cache::default();
        cache.state.lock().unwrap().entries.push((
            "herald-state-00000000000000000003".to_owned(),
            Some(b"not json".to_vec()),
        ));
        let error = pollster::block_on(store(&cache).entry(&key("a"))).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("herald-state-00000000000000000003"),
            "{error}"
        );
        let path = format!("ledger/{}.json", key("a"));
        let unencoded =
            json!({ "number": 3, "records": { path.clone(): { "written": 3, "bytes": "!!" } } });
        cache.state.lock().unwrap().entries.push((
            "herald-state-00000000000000000004".to_owned(),
            Some(unencoded.to_string().into_bytes()),
        ));
        let error = pollster::block_on(store(&cache).entry(&key("a"))).unwrap_err();
        assert!(error.to_string().contains(&path), "{error}");
    }

    #[test]
    fn a_write_or_removal_naming_a_record_since_replaced_changes_nothing() {
        use herald_records::{Records, Wrote};
        let cache = Cache::default();
        let store = store(&cache);
        let records = store.records();
        assert_eq!(
            pollster::block_on(records.remove("pins/r.json", "1")),
            Ok(Wrote::Written)
        );
        assert_eq!(
            pollster::block_on(records.write("pins/r.json", b"one", None)),
            Ok(Wrote::Written)
        );
        assert_eq!(
            pollster::block_on(records.write("pins/r.json", b"two", Some("7"))),
            Ok(Wrote::Conflict)
        );
        assert_eq!(
            pollster::block_on(records.remove("pins/r.json", "7")),
            Ok(Wrote::Conflict)
        );
        assert_eq!(
            pollster::block_on(records.read("pins/r.json"))
                .unwrap()
                .map(|blob| blob.bytes),
            Some(b"one".to_vec())
        );
    }

    struct Unreachable;

    impl Http for Unreachable {
        async fn send(&self, _: Request) -> Result<Response, HttpError> {
            Err(HttpError::TimedOut)
        }
    }

    #[test]
    fn an_unreachable_cache_is_an_unavailable_store() {
        let cache = ActionsCache::new(
            "https://results.example/",
            Secret::new("ACTIONS_RUNTIME_TOKEN", "t"),
            Some("state"),
        );
        assert_eq!(cache.results(), "https://results.example");
        let store = cache.store(Unreachable, Box::new(|_| Ok(Unreachable)));
        assert_eq!(
            pollster::block_on(store.entry(&key("a"))),
            Err(unavailable("the host did not answer in time"))
        );
        assert!(format!("{:?}", store.records()).contains("CacheRecords"));
    }
}
