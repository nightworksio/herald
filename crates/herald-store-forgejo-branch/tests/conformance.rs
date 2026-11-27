//! The forgejo-branch store against a model of Forgejo's contents, branches
//! and Git Trees APIs, answering as Forgejo's own handlers do, through the
//! conformance suite every store passes.

#[cfg(test)]
#[allow(
    clippy::unused_async_trait_impl,
    reason = "the model answers at once, and is written as the async trait it fakes"
)]
mod conformance {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::{Arc, Mutex};

    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use herald_core::{Identity, Key, RouteId};
    use herald_ports::{
        Http, HttpError, Ledger, Method, Pins, Request, Response, Schedules, Secret, StoreError,
        Subscriptions, Version, Written,
    };
    use herald_records::RecordBackend;
    use herald_store_forgejo_branch::{ForgejoBranch, ForgejoBranchStore};
    use herald_testing::store_conformance;
    use serde_json::{Value, json};

    const API: &str = "https://forge.example/api/v1";
    const PREFIX: &str = "https://forge.example/api/v1/repos/o/r";
    const HEAD: &str = "head";
    const TREE: &str = "tree:";

    #[derive(Debug)]
    struct Model {
        branch: bool,
        empty: bool,
        files: BTreeMap<String, (String, String)>,
        made: u64,
        requests: Vec<String>,
        pushed_late: u32,
        fail: Option<(String, u16, String)>,
        page: usize,
        counted: Option<usize>,
        pruned: bool,
    }

    impl Default for Model {
        fn default() -> Self {
            Self {
                branch: true,
                empty: false,
                files: BTreeMap::new(),
                made: 0,
                requests: Vec::new(),
                pushed_late: 0,
                fail: None,
                page: 1000,
                counted: None,
                pruned: false,
            }
        }
    }

    #[derive(Debug, Clone, Default)]
    struct Forgejo {
        model: Arc<Mutex<Model>>,
    }

    impl Forgejo {
        fn of(model: Model) -> Self {
            Self {
                model: Arc::new(Mutex::new(model)),
            }
        }

        fn requests(&self) -> Vec<String> {
            self.model.lock().unwrap().requests.clone()
        }

        fn with(&self, change: impl FnOnce(&mut Model)) {
            change(&mut self.model.lock().unwrap());
        }
    }

    fn answer(status: u16, body: &Value) -> Response {
        Response {
            status,
            headers: Vec::new(),
            body: if body.is_null() {
                Vec::new()
            } else {
                body.to_string().into_bytes()
            },
        }
    }

    impl Model {
        fn sha(&mut self) -> String {
            self.made += 1;
            format!("sha{}", self.made)
        }

        fn get(&self, path: &str) -> Response {
            let Some(path) = path.strip_suffix("?ref=herald-state") else {
                return answer(400, &json!({}));
            };
            match self.files.get(path) {
                Some((sha, content)) if self.branch => answer(
                    200,
                    &json!({
                        "type": "file",
                        "sha": sha,
                        "size": STANDARD.decode(content).unwrap().len(),
                        "content": content,
                    }),
                ),
                _ => answer(404, &json!({ "message": "object does not exist" })),
            }
        }

        fn head(&self, branch: &str) -> Response {
            if self.branch && branch == "herald-state" {
                answer(200, &json!({ "name": branch, "commit": { "id": HEAD } }))
            } else {
                answer(404, &json!({ "message": "branch does not exist" }))
            }
        }

        fn inside(&self, directory: &str) -> Vec<Value> {
            let prefix = if directory.is_empty() {
                String::new()
            } else {
                format!("{directory}/")
            };
            let mut entries = BTreeSet::new();
            for file in self.files.keys() {
                if let Some(rest) = file.strip_prefix(&prefix) {
                    entries.insert(match rest.split_once('/') {
                        Some((name, _)) => (name.to_owned(), "tree"),
                        None => (rest.to_owned(), "blob"),
                    });
                }
            }
            entries
                .into_iter()
                .map(|(name, kind)| {
                    let sha = if kind == "tree" {
                        format!("{TREE}{prefix}{name}")
                    } else {
                        format!("blob:{prefix}{name}")
                    };
                    json!({ "path": name, "type": kind, "sha": sha, "mode": "100644" })
                })
                .collect()
        }

        fn tree(&self, asked: &str) -> Response {
            let Some((sha, query)) = asked.split_once('?') else {
                return answer(400, &json!({}));
            };
            let Some(page) = query
                .strip_prefix("page=")
                .and_then(|rest| rest.strip_suffix("&per_page=1000"))
                .and_then(|page| page.parse::<usize>().ok())
            else {
                return answer(400, &json!({}));
            };
            if self.pruned {
                return answer(404, &json!({ "message": "sha not found" }));
            }
            let directory = match sha {
                HEAD => "",
                _ => match sha.strip_prefix(TREE) {
                    Some(directory) => directory,
                    None => return answer(404, &json!({ "message": "sha not found" })),
                },
            };
            let entries = self.inside(directory);
            let start = self.page * (page - 1);
            let listed: Value = if start >= entries.len() {
                Value::Null
            } else {
                Value::Array(entries[start..entries.len().min(start + self.page)].to_vec())
            };
            answer(
                200,
                &json!({
                    "sha": sha,
                    "tree": listed,
                    "truncated": entries.len() > self.page,
                    "page": page,
                    "total_count": self.counted.filter(|_| !directory.is_empty()).unwrap_or(entries.len()),
                }),
            )
        }

        fn create(&mut self, path: &str, body: &Value) -> Response {
            if !self.branch {
                if !self.empty {
                    return answer(404, &json!({ "message": "branch does not exist" }));
                }
                self.branch = true;
                self.empty = false;
            }
            if self.files.contains_key(path) {
                return answer(422, &json!({ "message": "repository file already exists" }));
            }
            if self.pushed_late > 0 {
                self.pushed_late -= 1;
                return answer(500, &json!({ "message": "non-fast-forward" }));
            }
            let sha = self.sha();
            let content = body["content"].as_str().unwrap_or_default().to_owned();
            self.files.insert(path.to_owned(), (sha, content));
            answer(201, &json!({ "content": {} }))
        }

        fn update(&mut self, path: &str, body: &Value) -> Response {
            if !self.branch {
                return answer(404, &json!({ "message": "branch does not exist" }));
            }
            let Some((current, _)) = self.files.get(path) else {
                return answer(500, &json!({ "message": "object does not exist" }));
            };
            if body["sha"].as_str() != Some(current.as_str()) {
                return answer(409, &json!({ "message": "sha does not match" }));
            }
            if self.pushed_late > 0 {
                self.pushed_late -= 1;
                return answer(500, &json!({ "message": "non-fast-forward" }));
            }
            let sha = self.sha();
            let content = body["content"].as_str().unwrap_or_default().to_owned();
            self.files.insert(path.to_owned(), (sha, content));
            answer(200, &json!({ "content": {} }))
        }

        fn delete(&mut self, path: &str, body: &Value) -> Response {
            match self.files.get(path) {
                None => answer(404, &json!({})),
                Some((sha, _)) if Some(sha.as_str()) != body["sha"].as_str() => {
                    answer(400, &json!({ "message": "sha does not match" }))
                }
                Some(_) => {
                    self.files.remove(path);
                    answer(200, &json!({}))
                }
            }
        }
    }

    impl Http for Forgejo {
        async fn send(&self, request: Request) -> Result<Response, HttpError> {
            let mut model = self.model.lock().unwrap();
            let url = request.url.expose();
            let path = url.strip_prefix(PREFIX).unwrap_or(&url).to_owned();
            model
                .requests
                .push(format!("{} {path}", request.method.as_str()));
            if let Some((method, status, body)) = model.fail.clone()
                && method == request.method.as_str()
            {
                return Ok(answer(
                    status,
                    &serde_json::from_str(&body).unwrap_or(Value::Null),
                ));
            }
            assert_eq!(
                request
                    .headers
                    .iter()
                    .find(|(name, _)| name == "Authorization")
                    .map(|(_, value)| value.redacted()),
                Some("token «secret:GITHUB_TOKEN»".to_owned())
            );
            let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
            if request.method == Method::Get {
                if let Some(branch) = path.strip_prefix("/branches/") {
                    return Ok(model.head(branch));
                }
                if let Some(asked) = path.strip_prefix("/git/trees/") {
                    return Ok(model.tree(asked));
                }
            }
            let Some(file) = path.strip_prefix("/contents/").map(ToOwned::to_owned) else {
                return Ok(answer(400, &json!({})));
            };
            if request.method != Method::Get {
                assert_eq!(body["branch"], "herald-state");
                assert_eq!(body["message"], format!("herald: {file}"));
            }
            Ok(match request.method {
                Method::Get => model.get(&file),
                Method::Post => model.create(&file, &body),
                Method::Put => model.update(&file, &body),
                Method::Delete => model.delete(&file, &body),
                Method::Patch => answer(400, &json!({})),
            })
        }
    }

    fn store(forgejo: Forgejo) -> ForgejoBranchStore<Forgejo> {
        ForgejoBranch::new("o/r", Secret::new("GITHUB_TOKEN", "hunter2"), None, API).store(forgejo)
    }

    fn modelled(model: Model) -> ForgejoBranchStore<Forgejo> {
        store(Forgejo::of(model))
    }

    fn key() -> Key {
        Key::derive(&RouteId::new("r"), &Identity::new("i"))
    }

    fn unavailable(reason: &str) -> StoreError {
        StoreError::Unavailable {
            reason: reason.to_owned(),
        }
    }

    #[test]
    fn the_store_passes_the_conformance_suite_and_makes_its_branch_in_an_empty_repository() {
        let forgejo = modelled(Model {
            branch: false,
            empty: true,
            ..Model::default()
        });
        assert_eq!(pollster::block_on(store_conformance(&forgejo)), Ok(()));
        assert!(herald_ports::Store::writes_one_at_a_time(&forgejo));
    }

    #[test]
    fn a_repository_holding_other_branches_but_not_the_store_s_says_how_to_make_it() {
        let forgejo = modelled(Model {
            branch: false,
            ..Model::default()
        });
        assert_eq!(
            pollster::block_on(forgejo.pinned(&RouteId::new("r"))),
            Ok(None)
        );
        assert_eq!(pollster::block_on(forgejo.every_schedule()), Ok(Vec::new()));
        assert_eq!(
            pollster::block_on(forgejo.put_pinned(&RouteId::new("r"), "1")),
            Err(unavailable(
                "Forgejo found no repository o/r with a branch herald-state; make the branch once, with no history, such as with `git switch --orphan herald-state && git commit --allow-empty -m \"herald: the state branch\" && git push origin herald-state`"
            ))
        );
    }

    #[test]
    fn a_directory_is_listed_a_page_at_a_time_through_the_trees_of_its_path() {
        let files: BTreeMap<String, (String, String)> = (0..5)
            .map(|index| {
                (
                    format!("inboxes/k/i{index}.json"),
                    (format!("sha{index}"), String::new()),
                )
            })
            .chain([
                (
                    "inboxes/other/x.json".to_owned(),
                    ("s".to_owned(), String::new()),
                ),
                (
                    "inboxes/k/deeper/y.json".to_owned(),
                    ("s".to_owned(), String::new()),
                ),
                ("ledger/e.json".to_owned(), ("s".to_owned(), String::new())),
            ])
            .collect();
        let model = Forgejo::of(Model {
            files,
            page: 2,
            ..Model::default()
        });
        let forgejo = store(model.clone());
        let listed = pollster::block_on(forgejo.records().list("inboxes/k")).unwrap();
        assert_eq!(
            listed,
            (0..5)
                .map(|index| format!("inboxes/k/i{index}.json"))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            model.requests(),
            [
                "GET /branches/herald-state",
                "GET /git/trees/head?page=1&per_page=1000",
                "GET /git/trees/tree:inboxes?page=1&per_page=1000",
                "GET /git/trees/tree:inboxes/k?page=1&per_page=1000",
                "GET /git/trees/tree:inboxes/k?page=2&per_page=1000",
                "GET /git/trees/tree:inboxes/k?page=3&per_page=1000",
            ]
        );
        assert_eq!(
            pollster::block_on(forgejo.records().list("inboxes/other")),
            Ok(vec!["inboxes/other/x.json".to_owned()])
        );
        assert_eq!(
            pollster::block_on(forgejo.records().list("inboxes/none")),
            Ok(Vec::new())
        );
        assert_eq!(
            pollster::block_on(forgejo.records().list("ledger/e.json")),
            Ok(Vec::new()),
            "a file is no directory"
        );
    }

    #[test]
    fn a_listing_forgejo_answers_in_part_or_past_the_most_is_refused() {
        let files: BTreeMap<String, (String, String)> = (0..3)
            .map(|index| {
                (
                    format!("notes/n{index}.json"),
                    ("s".to_owned(), String::new()),
                )
            })
            .collect();
        let short = modelled(Model {
            files: files.clone(),
            counted: Some(4),
            ..Model::default()
        });
        assert_eq!(
            pollster::block_on(short.records().list("notes")),
            Err(unavailable("Forgejo listed notes only in part"))
        );
        let crowded = modelled(Model {
            files,
            counted: Some(100_001),
            ..Model::default()
        });
        assert_eq!(
            pollster::block_on(crowded.records().list("notes")),
            Err(unavailable(
                "notes holds 100001 entries on Forgejo, more than the 100000 herald lists"
            ))
        );
        let most = modelled(Model {
            files: BTreeMap::from([("notes/n.json".to_owned(), ("s".to_owned(), String::new()))]),
            counted: Some(100_000),
            ..Model::default()
        });
        assert_eq!(
            pollster::block_on(most.records().list("notes")),
            Err(unavailable("Forgejo listed notes only in part")),
            "100000 entries are listed, as far as Forgejo answers them"
        );
        let pruned = modelled(Model {
            pruned: true,
            ..Model::default()
        });
        assert_eq!(
            pollster::block_on(pruned.records().list("notes")),
            Err(unavailable("Forgejo answered 404")),
            "a tree of the branch's head that Forgejo cannot find is no empty directory"
        );
    }

    #[test]
    fn each_write_of_a_record_counts_its_version_up_by_one() {
        let forgejo = store(Forgejo::default());
        let entry = herald_core::Entry::new(key());
        let versions: Vec<_> = [None, Some(1), Some(2)]
            .into_iter()
            .map(|expected| pollster::block_on(forgejo.put_entry(&entry, expected.map(Version))))
            .collect();
        assert_eq!(
            versions,
            [1, 2, 3].map(|version| Ok(Written::Written(Version(version))))
        );
        assert_eq!(
            pollster::block_on(forgejo.put_entry(&entry, Some(Version(1)))),
            Ok(Written::Conflict),
            "a write expecting an earlier version is refused"
        );
    }

    #[test]
    fn a_write_naming_a_replaced_blob_is_refused_and_one_racing_the_branch_is_not_taken() {
        let forgejo = store(Forgejo::default());
        let path = format!("ledger/{}.json", key());
        let records = forgejo.records();
        assert_eq!(
            pollster::block_on(records.write(&path, b"first", None)),
            Ok(herald_records::Wrote::Written)
        );
        assert_eq!(
            pollster::block_on(records.write(&path, b"again", None)),
            Ok(herald_records::Wrote::Conflict)
        );
        let read = pollster::block_on(records.read(&path)).unwrap().unwrap();
        assert_eq!(read.bytes, b"first");
        assert_eq!(
            pollster::block_on(records.write(&path, b"second", Some(&read.token))),
            Ok(herald_records::Wrote::Written)
        );
        assert_eq!(
            pollster::block_on(records.write(&path, b"lost", Some(&read.token))),
            Ok(herald_records::Wrote::Conflict)
        );
        assert_eq!(
            pollster::block_on(records.remove(&path, &read.token)),
            Ok(herald_records::Wrote::Conflict)
        );
        assert_eq!(
            pollster::block_on(records.write("ledger/gone.json", b"x", Some("sha1"))),
            Err(unavailable("Forgejo answered 500"))
        );
        let late = modelled(Model {
            pushed_late: 1,
            ..Model::default()
        });
        assert_eq!(
            pollster::block_on(late.put_pinned(&RouteId::new("r"), "1")),
            Err(unavailable("Forgejo answered 500"))
        );
        assert_eq!(
            pollster::block_on(late.pinned(&RouteId::new("r"))),
            Ok(None)
        );
        let model = Forgejo::of(Model {
            files: BTreeMap::from([(path.clone(), ("s".to_owned(), STANDARD.encode("x")))]),
            ..Model::default()
        });
        let gone = store(model.clone());
        model.with(|model| model.branch = false);
        assert_eq!(
            pollster::block_on(gone.records().write(&path, b"y", Some("s"))),
            Ok(herald_records::Wrote::Conflict),
            "a branch gone since the read is a record changed"
        );
    }

    fn failing(method: &str, status: u16, body: &str) -> ForgejoBranchStore<Forgejo> {
        modelled(Model {
            fail: Some((method.to_owned(), status, body.to_owned())),
            ..Model::default()
        })
    }

    #[test]
    fn what_forgejo_refuses_or_garbles_says_so_without_the_token() {
        let entry_path = format!("ledger/{}.json", key());
        for (body, says) in [
            ("[]", "expected struct File"),
            (
                r#"{"type":"dir","sha":"s","size":0,"content":null}"#,
                "Forgejo holds a dir there, not a file",
            ),
            (
                r#"{"type":"file","sha":"s","size":2,"content":"!!"}"#,
                "Invalid",
            ),
            (
                r#"{"type":"file","sha":"s","size":20971520,"content":""}"#,
                "Forgejo answered 0 of its 20971520 bytes, as a file larger than its instance's [api] DEFAULT_MAX_BLOB_SIZE",
            ),
        ] {
            let error = pollster::block_on(failing("GET", 200, body).entry(&key())).unwrap_err();
            assert!(matches!(error, StoreError::Corrupt { .. }), "{error}");
            let error = error.to_string();
            assert!(error.contains(&entry_path), "{error}");
            assert!(error.contains(says), "{error}");
            assert!(!error.contains("hunter2"), "{error}");
        }
        assert_eq!(
            pollster::block_on(failing("GET", 500, "{}").entry(&key())),
            Err(unavailable("Forgejo answered 500"))
        );
        assert_eq!(
            pollster::block_on(failing("GET", 500, "{}").every_schedule()),
            Err(unavailable("Forgejo answered 500"))
        );
        assert!(matches!(
            pollster::block_on(failing("GET", 200, "{}").every_schedule()),
            Err(StoreError::Corrupt { .. })
        ));
        let entry = herald_core::Entry::new(key());
        for method in ["POST", "PUT"] {
            let model = Forgejo::default();
            let refusing = store(model.clone());
            let expected = if method == "PUT" {
                pollster::block_on(refusing.put_entry(&entry, None)).unwrap();
                Some(Version(1))
            } else {
                None
            };
            model.with(|model| {
                model.fail = Some((
                    method.to_owned(),
                    403,
                    r#"{"message":"token hunter2 may not push"}"#.to_owned(),
                ));
            });
            assert_eq!(
                pollster::block_on(refusing.put_entry(&entry, expected)),
                Err(unavailable("Forgejo answered 403"))
            );
        }
        let subscription = herald_core::Subscription {
            destination: herald_core::DestinationName::new("slack"),
            project: "p".to_owned(),
            subscriber: "u".to_owned(),
            inbox: None,
            pending: None,
        };
        let deleting = failing("DELETE", 403, "{}");
        pollster::block_on(deleting.subscribe(&subscription)).unwrap();
        assert_eq!(
            pollster::block_on(deleting.unsubscribe(&subscription)),
            Err(unavailable("Forgejo answered 403"))
        );
        let racing = failing("DELETE", 400, "{}");
        pollster::block_on(racing.subscribe(&subscription)).unwrap();
        assert_eq!(
            pollster::block_on(racing.unsubscribe(&subscription)),
            Err(unavailable(
                "subscriptions/slack/p/u.json kept changing while herald removed it"
            ))
        );
        let gone = failing("DELETE", 404, "{}");
        pollster::block_on(gone.subscribe(&subscription)).unwrap();
        assert_eq!(pollster::block_on(gone.unsubscribe(&subscription)), Ok(()));
    }

    struct Unreachable;

    impl Http for Unreachable {
        async fn send(&self, _: Request) -> Result<Response, HttpError> {
            Err(HttpError::TimedOut)
        }
    }

    #[test]
    fn an_unreachable_forgejo_is_an_unavailable_store() {
        let branch = ForgejoBranch::new(
            "o/r",
            Secret::new("T", "t"),
            Some("state"),
            "https://forge.example/api/v1/",
        );
        assert_eq!(branch.api(), API);
        let store = branch.store(Unreachable);
        assert_eq!(
            pollster::block_on(store.entry(&key())),
            Err(unavailable("the host did not answer in time"))
        );
    }
}
