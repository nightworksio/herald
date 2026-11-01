//! The git-branch store against a model of GitHub's contents API, through
//! the conformance suite every store passes.

#[cfg(test)]
#[allow(
    clippy::unused_async_trait_impl,
    reason = "the model answers at once, and is written as the async trait it fakes"
)]
mod conformance {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use herald_core::{Identity, Key, RouteId};
    use herald_ports::{Http, HttpError, Method, Request, Response, Secret, Store, StoreError};
    use herald_store_git_branch::{GitBranch, GitBranchStore};
    use herald_testing::store_conformance;
    use serde_json::{Value, json};

    const PREFIX: &str = "https://api.github.com/repos/o/r";

    #[derive(Debug, Default)]
    struct Model {
        branch: bool,
        files: BTreeMap<String, (String, String)>,
        made: u64,
        requests: Vec<String>,
        race: u32,
        fail: Option<(String, u16, String)>,
        refs: Option<u16>,
    }

    #[derive(Debug, Default)]
    struct GitHub {
        model: Mutex<Model>,
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
            if !self.branch {
                return answer(404, &json!({}));
            }
            if let Some((sha, content)) = self.files.get(path) {
                return answer(200, &json!({ "sha": sha, "content": content }));
            }
            let inside: Vec<Value> = self
                .files
                .keys()
                .filter(|file| file.starts_with(&format!("{path}/")))
                .map(|file| json!({ "path": file }))
                .collect();
            if inside.is_empty() {
                answer(404, &json!({}))
            } else {
                answer(200, &Value::Array(inside))
            }
        }

        fn put(&mut self, path: &str, body: &Value) -> Response {
            if !self.branch {
                return answer(404, &json!({ "message": "Branch herald-state not found" }));
            }
            if self.race > 0 {
                self.race -= 1;
                return answer(409, &json!({ "message": "is at another sha" }));
            }
            let current = self.files.get(path).map(|(sha, _)| sha.clone());
            match (current, body.get("sha").and_then(Value::as_str)) {
                (Some(current), Some(given)) if current != given => answer(409, &json!({})),
                (Some(_), None) => answer(422, &json!({ "message": "sha wasn't supplied" })),
                (None, Some(_)) => answer(409, &json!({})),
                _ => {
                    let sha = self.sha();
                    let content = body["content"].as_str().unwrap_or_default().to_owned();
                    self.files.insert(path.to_owned(), (sha, content));
                    answer(201, &json!({ "content": {} }))
                }
            }
        }

        fn delete(&mut self, path: &str, body: &Value) -> Response {
            match self.files.get(path) {
                None => answer(404, &json!({})),
                Some((sha, _)) if Some(sha.as_str()) != body["sha"].as_str() => {
                    answer(409, &json!({}))
                }
                Some(_) => {
                    self.files.remove(path);
                    answer(200, &json!({}))
                }
            }
        }

        fn git(&mut self, path: &str) -> Response {
            match path {
                "/git/trees" | "/git/commits" => {
                    let sha = self.sha();
                    answer(201, &json!({ "sha": sha }))
                }
                "/git/refs" => {
                    let status = self.refs.unwrap_or(if self.branch { 422 } else { 201 });
                    self.branch = true;
                    answer(status, &json!({}))
                }
                _ => answer(400, &json!({})),
            }
        }
    }

    impl Http for GitHub {
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
                Some("Bearer «secret:GITHUB_TOKEN»".to_owned())
            );
            let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
            let file = path.strip_prefix("/contents/").map(ToOwned::to_owned);
            Ok(match (request.method, file) {
                (Method::Get, Some(file)) => model.get(&file),
                (Method::Put, Some(file)) => model.put(&file, &body),
                (Method::Delete, Some(file)) => model.delete(&file, &body),
                (Method::Post, None) => model.git(&path),
                _ => answer(400, &json!({})),
            })
        }
    }

    fn store(github: GitHub) -> GitBranchStore<GitHub> {
        GitBranchStore::new(
            GitBranch::new("o/r", Secret::new("GITHUB_TOKEN", "hunter2"), None, None),
            github,
        )
    }

    fn key() -> Key {
        Key::derive(&RouteId::new("r"), &Identity::new("i"))
    }

    #[test]
    fn the_store_passes_the_conformance_suite_and_makes_its_branch_on_first_write() {
        let github = store(GitHub::default());
        assert_eq!(pollster::block_on(store_conformance(&github)), Ok(()));
    }

    #[test]
    fn each_write_of_a_record_counts_its_version_up_by_one() {
        let github = store(GitHub::default());
        let entry = herald_core::Entry::new(key());
        let versions: Vec<_> = [None, Some(1), Some(2)]
            .into_iter()
            .map(|expected| {
                pollster::block_on(github.put_entry(&entry, expected.map(herald_ports::Version)))
            })
            .collect();
        assert_eq!(
            versions,
            [1, 2, 3].map(
                |version| Ok(herald_ports::Written::Written(herald_ports::Version(
                    version
                )))
            )
        );
    }

    #[test]
    fn a_write_that_loses_a_race_to_an_unrelated_write_is_tried_again() {
        let model = Model {
            branch: true,
            race: 2,
            ..Model::default()
        };
        let github = store(GitHub {
            model: Mutex::new(model),
        });
        let entry = herald_core::Entry::new(key());
        assert!(matches!(
            pollster::block_on(github.put_entry(&entry, None)),
            Ok(herald_ports::Written::Written(_))
        ));
        let model = Model {
            branch: true,
            race: 3,
            ..Model::default()
        };
        let github = store(GitHub {
            model: Mutex::new(model),
        });
        assert_eq!(
            pollster::block_on(github.put_entry(&entry, None)),
            Ok(herald_ports::Written::Conflict)
        );
        let model = Model {
            branch: true,
            race: 3,
            ..Model::default()
        };
        let github = store(GitHub {
            model: Mutex::new(model),
        });
        assert_eq!(
            pollster::block_on(github.put_pinned(&RouteId::new("r"), "1")),
            Err(StoreError::Unavailable {
                reason: "pins/r.json kept changing while herald wrote it".to_owned()
            })
        );
    }

    fn failing(method: &str, status: u16, body: &str) -> GitBranchStore<GitHub> {
        let model = Model {
            branch: true,
            fail: Some((method.to_owned(), status, body.to_owned())),
            ..Model::default()
        };
        store(GitHub {
            model: Mutex::new(model),
        })
    }

    #[test]
    fn what_github_refuses_or_garbles_says_so() {
        let unavailable = |reason: &str| StoreError::Unavailable {
            reason: reason.to_owned(),
        };
        assert_eq!(
            pollster::block_on(failing("GET", 500, "{}").entry(&key())),
            Err(unavailable("GitHub answered 500"))
        );
        assert_eq!(
            pollster::block_on(failing("GET", 500, "{}").schedules()),
            Err(unavailable("GitHub answered 500"))
        );
        assert!(matches!(
            pollster::block_on(failing("GET", 200, "[]").entry(&key())),
            Err(StoreError::Corrupt { .. })
        ));
        assert!(matches!(
            pollster::block_on(failing("GET", 200, r#"{"sha":"s","content":"!!"}"#).entry(&key())),
            Err(StoreError::Corrupt { .. })
        ));
        let not_a_record = STANDARD.encode("[]");
        assert!(matches!(
            pollster::block_on(
                failing(
                    "GET",
                    200,
                    &format!(r#"{{"sha":"s","content":"{not_a_record}"}}"#)
                )
                .entry(&key())
            ),
            Err(StoreError::Corrupt { .. })
        ));
        let wrong_shape = STANDARD.encode(r#"{"version":1,"record":7}"#);
        let error = pollster::block_on(
            failing(
                "GET",
                200,
                &format!(r#"{{"sha":"s","content":"{wrong_shape}"}}"#),
            )
            .entry(&key()),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains(&format!("ledger/{}.json", key())),
            "{error}"
        );
        assert!(matches!(
            pollster::block_on(failing("GET", 200, "{}").schedules()),
            Err(StoreError::Corrupt { .. })
        ));
        let entry = herald_core::Entry::new(key());
        assert_eq!(
            pollster::block_on(failing("PUT", 403, "{}").put_entry(&entry, None)),
            Err(unavailable("GitHub answered 403"))
        );
        let subscription = herald_core::Subscription {
            destination: herald_core::DestinationName::new("slack"),
            project: "p".to_owned(),
            subscriber: "u".to_owned(),
        };
        let deleting = failing("DELETE", 403, "{}");
        pollster::block_on(deleting.subscribe(&subscription)).unwrap();
        assert_eq!(
            pollster::block_on(deleting.unsubscribe(&subscription)),
            Err(unavailable("GitHub answered 403"))
        );
        let racing = failing("DELETE", 409, "{}");
        pollster::block_on(racing.subscribe(&subscription)).unwrap();
        assert_eq!(
            pollster::block_on(racing.unsubscribe(&subscription)),
            Err(unavailable(
                "subscriptions/slack/p/u.json kept changing while herald removed it"
            ))
        );
    }

    #[test]
    fn a_branch_that_cannot_be_made_says_so() {
        let cases = [
            ("{}", 403, "GitHub answered 403"),
            ("not json", 201, "could not be read"),
        ];
        for (body, status, reason) in cases {
            let model = Model {
                fail: Some(("POST".to_owned(), status, body.to_owned())),
                ..Model::default()
            };
            let github = store(GitHub {
                model: Mutex::new(model),
            });
            let error = pollster::block_on(github.put_pinned(&RouteId::new("r"), "1")).unwrap_err();
            assert!(error.to_string().contains(reason), "{error}");
        }
    }

    #[test]
    fn a_branch_reference_github_refuses_says_so() {
        let model = Model {
            refs: Some(403),
            ..Model::default()
        };
        let github = store(GitHub {
            model: Mutex::new(model),
        });
        assert_eq!(
            pollster::block_on(github.put_pinned(&RouteId::new("r"), "1")),
            Err(StoreError::Unavailable {
                reason: "GitHub answered 403".to_owned()
            })
        );
    }

    struct Unreachable;

    impl Http for Unreachable {
        async fn send(&self, _: Request) -> Result<Response, HttpError> {
            Err(HttpError::TimedOut)
        }
    }

    #[test]
    fn an_unreachable_github_is_an_unavailable_store() {
        let branch = GitBranch::new(
            "o/r",
            Secret::new("T", "t"),
            Some("state"),
            Some("https://ghe.example/api/v3/"),
        );
        assert_eq!(branch.api(), "https://ghe.example/api/v3");
        let store = GitBranchStore::new(branch, Unreachable);
        assert_eq!(
            pollster::block_on(store.entry(&key())),
            Err(StoreError::Unavailable {
                reason: "the host did not answer in time".to_owned()
            })
        );
    }
}
