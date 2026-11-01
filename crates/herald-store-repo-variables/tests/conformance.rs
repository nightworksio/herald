//! The repository-variables store against a model of GitHub's variables API,
//! through the conformance suite every store passes, and through the lock
//! another run holds or abandoned.

#[cfg(test)]
#[allow(
    clippy::unused_async_trait_impl,
    reason = "the model answers at once, and is written as the async trait it fakes"
)]
mod conformance {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use herald_core::{DestinationName, Entry, Identity, Key, RouteId, Subscription};
    use herald_ports::{
        Http, HttpError, Method, Request, Response, Secret, Store, StoreError, Written,
    };
    use herald_records::{Records, Wrote};
    use herald_store_repo_variables::{RepoVariables, RepoVariablesStore};
    use herald_testing::store_conformance;
    use serde_json::{Value, json};

    const PREFIX: &str = "https://api.github.com/repos/o/r/actions/variables";
    const NOW: &str = "Sun, 01 Nov 2026 12:00:00 GMT";
    const LOCK: &str = "HERALDLOCK";

    #[derive(Debug, Default)]
    struct Model {
        variables: BTreeMap<String, (String, Option<String>)>,
        fail: Option<(String, u16)>,
        undated: bool,
        lock_refused_once: bool,
    }

    #[derive(Debug, Default)]
    struct GitHub {
        model: Mutex<Model>,
    }

    fn answer(status: u16, body: &Value, dated: bool) -> Response {
        Response {
            status,
            headers: if dated {
                vec![("Date".to_owned(), NOW.to_owned())]
            } else {
                Vec::new()
            },
            body: if body.is_null() {
                Vec::new()
            } else {
                body.to_string().into_bytes()
            },
        }
    }

    fn shown(name: &str, value: &str, created: Option<&String>) -> Value {
        json!({ "name": name, "value": value, "created_at": created })
    }

    impl Model {
        fn answer(&mut self, method: Method, path: &str, body: &Value) -> Response {
            let dated = !self.undated;
            if let Some((failing, status)) = &self.fail
                && (failing == method.as_str() || failing == path)
            {
                return answer(*status, &json!({ "message": "no" }), dated);
            }
            if let Some(query) = path.strip_prefix('?') {
                let page: usize = query
                    .rsplit('=')
                    .next()
                    .and_then(|page| page.parse().ok())
                    .unwrap_or(1);
                let all: Vec<Value> = self
                    .variables
                    .iter()
                    .map(|(name, (value, created))| shown(name, value, created.as_ref()))
                    .collect();
                let listed: Vec<Value> = all.into_iter().skip((page - 1) * 30).take(30).collect();
                return answer(
                    200,
                    &json!({ "total_count": 0, "variables": listed }),
                    dated,
                );
            }
            let name = path.trim_start_matches('/').to_ascii_uppercase();
            match method {
                Method::Get => match self.variables.get(&name) {
                    Some((value, created)) => {
                        answer(200, &shown(&name, value, created.as_ref()), dated)
                    }
                    None => answer(404, &Value::Null, dated),
                },
                Method::Post => {
                    let name = body["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_ascii_uppercase();
                    if name == LOCK && self.lock_refused_once {
                        self.lock_refused_once = false;
                        return answer(409, &json!({ "message": "Already exists" }), dated);
                    }
                    if self.variables.contains_key(&name) {
                        return answer(409, &json!({ "message": "Already exists" }), dated);
                    }
                    let value = body["value"].as_str().unwrap_or_default().to_owned();
                    self.variables
                        .insert(name, (value, Some("2026-11-01T11:59:00Z".to_owned())));
                    answer(201, &Value::Null, dated)
                }
                Method::Patch => match self.variables.get_mut(&name) {
                    Some(held) => {
                        held.0 = body["value"].as_str().unwrap_or_default().to_owned();
                        answer(204, &Value::Null, dated)
                    }
                    None => answer(404, &Value::Null, dated),
                },
                Method::Delete => {
                    let status = if self.variables.remove(&name).is_some() {
                        204
                    } else {
                        404
                    };
                    answer(status, &Value::Null, dated)
                }
                Method::Put => answer(400, &Value::Null, dated),
            }
        }
    }

    impl Http for GitHub {
        async fn send(&self, request: Request) -> Result<Response, HttpError> {
            assert_eq!(
                request
                    .headers
                    .iter()
                    .find(|(name, _)| name == "Authorization")
                    .map(|(_, value)| value.redacted()),
                Some("Bearer «secret:HERALD_STATE_TOKEN»".to_owned())
            );
            let url = request.url.expose();
            let path = url.strip_prefix(PREFIX).unwrap_or(&url).to_owned();
            let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
            Ok(self
                .model
                .lock()
                .unwrap()
                .answer(request.method, &path, &body))
        }
    }

    fn store(model: Model) -> RepoVariablesStore<GitHub> {
        RepoVariables::new("o/r", Secret::new("HERALD_STATE_TOKEN", "t"), None).store(GitHub {
            model: Mutex::new(model),
        })
    }

    fn key() -> Key {
        Key::derive(&RouteId::new("r"), &Identity::new("i"))
    }

    fn locked(created: Option<&str>) -> Model {
        let mut model = Model::default();
        model.variables.insert(
            LOCK.to_owned(),
            ("held".to_owned(), created.map(ToOwned::to_owned)),
        );
        model
    }

    #[test]
    fn the_store_passes_the_conformance_suite_releasing_every_lock_it_takes() {
        let store = store(Model::default());
        assert_eq!(pollster::block_on(store_conformance(&store)), Ok(()));
    }

    #[test]
    fn every_page_of_variables_is_read_and_others_variables_are_left_alone() {
        let mut model = Model::default();
        model
            .variables
            .insert("DEPLOY_TARGET".to_owned(), ("prod".to_owned(), None));
        let store = store(model);
        for subscriber in 0..35 {
            let subscription = Subscription {
                destination: DestinationName::new("slack"),
                project: "p".to_owned(),
                subscriber: subscriber.to_string(),
            };
            pollster::block_on(store.subscribe(&subscription)).unwrap();
        }
        let subscribers =
            pollster::block_on(store.subscribers(&DestinationName::new("slack"), "p")).unwrap();
        assert_eq!(subscribers.len(), 35);
    }

    #[test]
    fn a_lock_another_run_holds_stops_a_write_and_an_abandoned_one_is_taken() {
        let entry = Entry::new(key());
        let held = store(locked(Some("2026-11-01T11:55:00Z")));
        assert_eq!(
            pollster::block_on(held.put_entry(&entry, None)),
            Ok(Written::Conflict)
        );
        let abandoned = store(locked(Some("2026-11-01T11:40:00Z")));
        assert_eq!(
            pollster::block_on(abandoned.put_entry(&entry, None)),
            Ok(Written::Written(herald_ports::Version(1)))
        );
        let mut undated = locked(Some("2026-11-01T11:40:00Z"));
        undated.undated = true;
        assert_eq!(
            pollster::block_on(store(undated).put_entry(&entry, None)),
            Ok(Written::Conflict)
        );
        assert_eq!(
            pollster::block_on(store(locked(None)).put_entry(&entry, None)),
            Ok(Written::Conflict)
        );
        let released = store(Model {
            lock_refused_once: true,
            ..Model::default()
        });
        assert_eq!(
            pollster::block_on(released.put_entry(&entry, None)),
            Ok(Written::Written(herald_ports::Version(1)))
        );
    }

    #[test]
    fn a_write_or_removal_naming_a_record_since_replaced_changes_nothing() {
        let store = store(Model::default());
        let records = store.records();
        assert_eq!(
            pollster::block_on(records.remove("pins/r.json", "x")),
            Ok(Wrote::Written)
        );
        assert_eq!(
            pollster::block_on(records.write("pins/r.json", b"one", None)),
            Ok(Wrote::Written)
        );
        assert_eq!(
            pollster::block_on(records.write("pins/r.json", b"two", None)),
            Ok(Wrote::Conflict)
        );
        assert_eq!(
            pollster::block_on(records.remove("pins/r.json", "two")),
            Ok(Wrote::Conflict)
        );
        assert_eq!(
            pollster::block_on(records.write("pins/r.json", b"two", Some("one"))),
            Ok(Wrote::Written)
        );
    }

    fn failing(at: &str, status: u16) -> RepoVariablesStore<GitHub> {
        store(Model {
            fail: Some((at.to_owned(), status)),
            ..Model::default()
        })
    }

    #[test]
    fn what_github_refuses_or_garbles_says_so() {
        let entry = Entry::new(key());
        assert_eq!(
            pollster::block_on(failing("POST", 403).put_entry(&entry, None)),
            Err(StoreError::Unavailable {
                reason: "GitHub answered 403: the token cannot write the repository's variables; give the store a token with the Variables permission".to_owned()
            })
        );
        assert_eq!(
            pollster::block_on(failing("GET", 500).entry(&key())),
            Err(StoreError::Unavailable {
                reason: "GitHub answered 500".to_owned()
            })
        );
        assert!(matches!(
            pollster::block_on(failing("GET", 200).entry(&key())),
            Err(StoreError::Corrupt { .. })
        ));
        assert!(matches!(
            pollster::block_on(failing("GET", 200).schedules()),
            Err(StoreError::Corrupt { .. })
        ));
        assert!(matches!(
            pollster::block_on(failing("GET", 502).schedules()),
            Err(StoreError::Unavailable { .. })
        ));
        assert!(matches!(
            pollster::block_on(failing("PATCH", 500).put_pinned(&RouteId::new("r"), "1")),
            Ok(())
        ));
        let pinned = failing("PATCH", 500);
        pollster::block_on(pinned.put_pinned(&RouteId::new("r"), "1")).unwrap();
        assert!(matches!(
            pollster::block_on(pinned.put_pinned(&RouteId::new("r"), "2")),
            Err(StoreError::Unavailable { .. })
        ));
        let unlocking = failing(&format!("/{LOCK}"), 500);
        assert!(matches!(
            pollster::block_on(unlocking.put_entry(&entry, None)),
            Err(StoreError::Unavailable { .. })
        ));
    }

    struct Unreachable;

    impl Http for Unreachable {
        async fn send(&self, _: Request) -> Result<Response, HttpError> {
            Err(HttpError::TimedOut)
        }
    }

    #[test]
    fn an_unreachable_github_is_an_unavailable_store() {
        let variables = RepoVariables::new(
            "o/r",
            Secret::new("T", "t"),
            Some("https://ghe.example/api/v3/"),
        );
        assert_eq!(variables.api(), "https://ghe.example/api/v3");
        assert_eq!(
            pollster::block_on(variables.store(Unreachable).entry(&key())),
            Err(StoreError::Unavailable {
                reason: "the host did not answer in time".to_owned()
            })
        );
    }
}
