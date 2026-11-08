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
    /// The holder this run's store writes into the lock.
    const THIS_RUN: &str = "run-1";
    /// The holder another run writes into the lock.
    const OTHER_RUN: &str = "run-2";
    /// When a variable written in the model was created, a minute before
    /// [`NOW`].
    const RECENT: &str = "2026-11-01T11:59:00Z";

    /// What another run does to the lock once this run has read it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum OnRead {
        Replaced,
        Released,
    }

    /// What befalls the lock once this run has written a record under it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum OnWrite {
        Stolen,
        Unreadable,
    }

    #[derive(Debug, Default)]
    struct Model {
        variables: BTreeMap<String, (String, Option<String>)>,
        fail: Option<(String, u16)>,
        undated: bool,
        lock_refusals: usize,
        stolen: bool,
        on_read: Option<OnRead>,
        on_write: Option<OnWrite>,
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
                let total = all.len();
                let listed: Vec<Value> = all.into_iter().skip((page - 1) * 30).take(30).collect();
                return answer(
                    200,
                    &json!({ "total_count": total, "variables": listed }),
                    dated,
                );
            }
            let name = path.trim_start_matches('/').to_ascii_uppercase();
            match method {
                Method::Get => self.read(&name, dated),
                Method::Post => self.posted(body, dated),
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

        fn read(&mut self, name: &str, dated: bool) -> Response {
            let read = match self.variables.get(name) {
                Some((value, created)) => answer(200, &shown(name, value, created.as_ref()), dated),
                None => answer(404, &Value::Null, dated),
            };
            if name == LOCK {
                match self.on_read.take() {
                    Some(OnRead::Replaced) => {
                        self.variables.insert(
                            LOCK.to_owned(),
                            (OTHER_RUN.to_owned(), Some(RECENT.to_owned())),
                        );
                    }
                    Some(OnRead::Released) => {
                        self.variables.remove(LOCK);
                    }
                    None => {}
                }
            }
            read
        }

        fn posted(&mut self, body: &Value, dated: bool) -> Response {
            let name = body["name"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_uppercase();
            if name == LOCK && self.lock_refusals > 0 {
                self.lock_refusals -= 1;
                return answer(409, &json!({ "message": "Already exists" }), dated);
            }
            if self.variables.contains_key(&name) {
                return answer(409, &json!({ "message": "Already exists" }), dated);
            }
            let mut value = body["value"].as_str().unwrap_or_default().to_owned();
            if name == LOCK && self.stolen {
                value = OTHER_RUN.to_owned();
            }
            let befalls = if name == LOCK {
                None
            } else {
                self.on_write.take()
            };
            self.variables
                .insert(name, (value, Some(RECENT.to_owned())));
            match befalls {
                Some(OnWrite::Stolen) => {
                    if let Some(lock) = self.variables.get_mut(LOCK) {
                        lock.0 = OTHER_RUN.to_owned();
                    }
                }
                Some(OnWrite::Unreadable) => self.fail = Some((format!("/{LOCK}"), 500)),
                None => {}
            }
            answer(201, &Value::Null, dated)
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

    struct Shared(std::sync::Arc<GitHub>);

    impl Http for Shared {
        async fn send(&self, request: Request) -> Result<Response, HttpError> {
            self.0.send(request).await
        }
    }

    fn store(model: Model) -> RepoVariablesStore<GitHub> {
        RepoVariables::new("o/r", Secret::new("HERALD_STATE_TOKEN", "t"), None).store(
            GitHub {
                model: Mutex::new(model),
            },
            THIS_RUN,
        )
    }

    /// A store over `model`, and the model, to change or read while the
    /// store runs.
    fn shared(model: Model) -> (std::sync::Arc<GitHub>, RepoVariablesStore<Shared>) {
        let github = std::sync::Arc::new(GitHub {
            model: Mutex::new(model),
        });
        let store = RepoVariables::new("o/r", Secret::new("HERALD_STATE_TOKEN", "t"), None)
            .store(Shared(std::sync::Arc::clone(&github)), THIS_RUN);
        (github, store)
    }

    /// Who holds the lock in `github`, where anyone does.
    fn holder(github: &GitHub) -> Option<String> {
        github
            .model
            .lock()
            .unwrap()
            .variables
            .get(LOCK)
            .map(|(value, _)| value.clone())
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
                inbox: None,
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
        let lapsing = store(locked(Some("2026-11-01T11:50:00Z")));
        assert_eq!(
            pollster::block_on(lapsing.put_entry(&entry, None)),
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
            lock_refusals: 1,
            ..Model::default()
        });
        assert_eq!(
            pollster::block_on(released.put_entry(&entry, None)),
            Ok(Written::Written(herald_ports::Version(1)))
        );
        let raced = store(Model {
            lock_refusals: 2,
            ..Model::default()
        });
        assert_eq!(
            pollster::block_on(raced.records().write("pins/r.json", b"one", None)),
            Ok(Wrote::Conflict)
        );
    }

    #[test]
    fn an_abandoned_lock_is_deleted_only_while_it_is_still_the_one_found() {
        let entry = Entry::new(key());
        let replaced = store(Model {
            on_read: Some(OnRead::Replaced),
            ..locked(Some("2026-11-01T11:40:00Z"))
        });
        assert_eq!(
            pollster::block_on(replaced.put_entry(&entry, None)),
            Ok(Written::Conflict)
        );
        let released = store(Model {
            on_read: Some(OnRead::Released),
            ..locked(Some("2026-11-01T11:40:00Z"))
        });
        assert_eq!(
            pollster::block_on(released.put_entry(&entry, None)),
            Ok(Written::Written(herald_ports::Version(1)))
        );
    }

    #[test]
    fn a_lock_that_changes_between_looks_is_never_taken_as_abandoned() {
        let entry = Entry::new(key());
        let mut undated = locked(None);
        undated.undated = true;
        let (github, store) = shared(undated);
        for look in 0..30 {
            assert_eq!(
                pollster::block_on(store.put_entry(&entry, None)),
                Ok(Written::Conflict),
                "{look}"
            );
            if let Some(lock) = github.model.lock().unwrap().variables.get_mut(LOCK) {
                lock.0 = format!("held-{look}");
            }
        }
    }

    #[test]
    fn a_lock_with_no_time_to_judge_it_by_is_taken_once_found_unchanged_often_enough() {
        let entry = Entry::new(key());
        let store = store(locked(None));
        assert_eq!(
            pollster::block_on(store.put_entry(&entry, None)),
            Ok(Written::Conflict)
        );
        let written = (0..10)
            .map(|_| pollster::block_on(store.put_entry(&entry, None)))
            .position(|written| written == Ok(Written::Written(herald_ports::Version(1))));
        assert!(written.is_some());
    }

    #[test]
    fn a_lock_taken_from_this_run_while_it_writes_is_left_and_one_unread_stops_the_write() {
        let entry = Entry::new(key());
        let stolen = store(Model {
            on_write: Some(OnWrite::Stolen),
            ..Model::default()
        });
        assert_eq!(
            pollster::block_on(stolen.put_entry(&entry, None)),
            Ok(Written::Written(herald_ports::Version(1)))
        );
        let later = Entry::new(Key::derive(&RouteId::new("r"), &Identity::new("later")));
        assert_eq!(
            pollster::block_on(stolen.put_entry(&later, None)),
            Ok(Written::Conflict)
        );
        let unread = store(Model {
            on_write: Some(OnWrite::Unreadable),
            ..Model::default()
        });
        assert!(matches!(
            pollster::block_on(unread.put_entry(&entry, None)),
            Err(StoreError::Unavailable { .. })
        ));
    }

    #[test]
    fn a_lock_is_released_only_where_it_is_still_this_run_s() {
        let entry = Entry::new(key());
        let (github, store) = shared(Model {
            stolen: true,
            ..Model::default()
        });
        assert_eq!(
            pollster::block_on(store.put_entry(&entry, None)),
            Ok(Written::Conflict)
        );
        assert_eq!(holder(&github).as_deref(), Some(OTHER_RUN));
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
            pollster::block_on(variables.store(Unreachable, THIS_RUN).entry(&key())),
            Err(StoreError::Unavailable {
                reason: "the host did not answer in time".to_owned()
            })
        );
    }
}
