//! The exchanges CI's dogfood job holds herald to (ADR-0010): each config in
//! `.github/dogfood` announces a sample release twice through the CLI, and
//! every request to the recorded-fixture server, with its answer, must be
//! the fixture committed beside the config. `HERALD_WRITE_DOGFOOD=1` writes
//! the fixtures instead.

#[cfg(test)]
#[allow(
    clippy::unused_async_trait_impl,
    reason = "the fake answers at once, and is written as the async trait it fakes"
)]
mod dogfood {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use herald::{Connection, DONE, run};
    use herald_ports::{Http, HttpError, Method, Request, Response};
    use serde_json::{Value, json};

    const FIXTURES: &str = "http://127.0.0.1:8787";
    const CHANNEL: &str = "1566184281406177531";
    const BOT: &str = "1566190000000000099";
    const POST: &str = "1566200000000000001";
    const TAG: &str = "v0.0.0-dogfood";

    #[derive(Debug, Default)]
    struct State {
        messages: Vec<Value>,
        cache: BTreeMap<String, Vec<u8>>,
        exchanges: Vec<Value>,
    }

    #[derive(Debug, Clone, Default)]
    struct Platform {
        state: Arc<Mutex<State>>,
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

    impl State {
        fn github(path: &str) -> (u16, Value) {
            match path {
                "/repos/nightworksio/herald" => {
                    (200, json!({ "id": 870_000_001, "name": "herald" }))
                }
                "/repos/nightworksio/herald/releases/tags/v0.0.0-dogfood" => (
                    200,
                    json!({
                        "tag_name": TAG,
                        "name": "herald v0.0.0-dogfood",
                        "body": "## Added\n\n- The dogfood run.",
                        "draft": false,
                        "prerelease": true,
                        "published_at": "2026-11-01T12:00:00Z",
                        "html_url": "https://github.com/nightworksio/herald/releases/tag/v0.0.0-dogfood",
                    }),
                ),
                _ => (404, json!({ "message": "Not Found" })),
            }
        }

        fn post(&mut self) -> Option<&mut Value> {
            self.messages.first_mut()
        }

        fn discord(&mut self, method: Method, path: &str, body: &Value) -> (u16, Value) {
            let channel = format!("/channels/{CHANNEL}");
            let thread = format!("/channels/{POST}/messages?limit=100");
            match (method, path) {
                (Method::Get, "/users/@me") => {
                    (200, json!({ "id": BOT, "username": "herald", "bot": true }))
                }
                (Method::Get, listed) if listed == format!("{channel}/messages?limit=100") => {
                    (200, Value::Array(self.messages.clone()))
                }
                (Method::Get, listed) if listed == thread => (200, json!([])),
                (Method::Get, pins) if pins == format!("{channel}/messages/pins?limit=50") => {
                    let items: Vec<Value> = self
                        .messages
                        .iter()
                        .filter(|message| message["pinned"] == true)
                        .map(|message| json!({ "message": message }))
                        .collect();
                    (200, json!({ "items": items }))
                }
                (Method::Post, posting) if posting == format!("{channel}/messages") => {
                    let mut message = body.clone();
                    message["id"] = json!(POST);
                    message["channel_id"] = json!(CHANNEL);
                    message["type"] = json!(0);
                    message["author"] = json!({ "id": BOT, "username": "herald", "bot": true });
                    message["flags"] = json!(0);
                    message["pinned"] = json!(false);
                    message["timestamp"] = json!("2026-11-01T12:00:01.000000+00:00");
                    self.messages.insert(0, message.clone());
                    (200, message)
                }
                (Method::Post, crossposted)
                    if crossposted == format!("{channel}/messages/{POST}/crosspost") =>
                {
                    let post = self.post().map(|post| {
                        post["flags"] = json!(1);
                        post.clone()
                    });
                    (200, post.unwrap_or_default())
                }
                (Method::Post, threaded)
                    if threaded == format!("{channel}/messages/{POST}/threads") =>
                {
                    let opened = json!({
                        "id": POST,
                        "type": 11,
                        "parent_id": CHANNEL,
                        "name": body["name"],
                        "thread_metadata": {
                            "archived": false,
                            "locked": false,
                            "create_timestamp": "2026-11-01T12:00:02.000000+00:00",
                        },
                    });
                    if let Some(post) = self.post() {
                        post["thread"] = opened.clone();
                    }
                    (200, opened)
                }
                (Method::Put, pinned) if pinned == format!("{channel}/messages/pins/{POST}") => {
                    if let Some(post) = self.post() {
                        post["pinned"] = json!(true);
                    }
                    (204, Value::Null)
                }
                _ => (404, json!({ "code": 0, "message": "404: Not Found" })),
            }
        }

        fn results(&mut self, method: &str, body: &Value) -> Response {
            let key = body["key"].as_str().unwrap_or_default().to_owned();
            match method {
                "GetCacheEntryDownloadURL" => match self.cache.keys().next_back() {
                    Some(latest) => answer(
                        200,
                        &json!({
                            "ok": true,
                            "signed_download_url": format!("https://blobs.example/{latest}"),
                            "matched_key": latest,
                        }),
                    ),
                    None => answer(200, &json!({ "ok": false })),
                },
                "CreateCacheEntry" => answer(
                    200,
                    &json!({ "ok": true, "signed_upload_url": format!("https://blobs.example/{key}") }),
                ),
                _ => answer(200, &json!({ "ok": true })),
            }
        }
    }

    impl Http for Platform {
        async fn send(&self, request: Request) -> Result<Response, HttpError> {
            let mut state = self.state.lock().unwrap();
            let url = request.url.expose();
            let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
            if let Some(method) = url.strip_prefix(
                "https://results.example/twirp/github.actions.results.api.v1.CacheService/",
            ) {
                return Ok(state.results(method, &body));
            }
            if let Some(key) = url.strip_prefix("https://blobs.example/") {
                if request.method == Method::Put {
                    state.cache.insert(key.to_owned(), request.body);
                    return Ok(answer(201, &Value::Null));
                }
                let held = state.cache.get(key).cloned().unwrap_or_default();
                return Ok(Response {
                    status: 200,
                    headers: Vec::new(),
                    body: held,
                });
            }
            let path = url.strip_prefix(FIXTURES).unwrap_or_default().to_owned();
            let (status, answered) = if let Some(github) = path.strip_prefix("/github") {
                State::github(github)
            } else {
                let discord = path.strip_prefix("/discord/api/v10").unwrap_or_default();
                state.discord(request.method, discord, &body)
            };
            state.exchanges.push(json!({
                "method": request.method.as_str(),
                "path": path,
                "body": body,
                "status": status,
                "response": answered,
            }));
            Ok(answer(status, &answered))
        }
    }

    fn dogfood() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.github/dogfood")
    }

    fn announced(store: &str) -> (Vec<String>, Value) {
        let platform = Platform::default();
        let vars: BTreeMap<String, String> = [
            ("GITHUB_REPOSITORY", "nightworksio/herald"),
            ("GITHUB_EVENT_NAME", "pull_request"),
            ("GITHUB_TOKEN", "the run's token"),
            ("HERALD_DISCORD_TOKEN", "ci-discord-token"),
            ("HERALD_MARKER_SECRET", "ci-marker-secret"),
            ("ACTIONS_RESULTS_URL", "https://results.example"),
            ("ACTIONS_RUNTIME_TOKEN", "ci-runtime-token"),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
        let config = dogfood()
            .join(format!("{store}.toml"))
            .display()
            .to_string();
        let args: Vec<String> = [
            "herald",
            "--config",
            &config,
            "announce",
            "--from",
            "github",
            "--release",
            TAG,
        ]
        .map(ToOwned::to_owned)
        .to_vec();
        let connect = |_: &str| Ok(Connection::new(platform.clone()));
        let outputs = (0..2)
            .map(|_| {
                let mut out = Vec::new();
                let mut err = Vec::new();
                let code = pollster::block_on(run(&args, &vars, &connect, &mut out, &mut err));
                let out = String::from_utf8(out).unwrap();
                assert_eq!(code, DONE, "{out}{}", String::from_utf8_lossy(&err));
                out
            })
            .collect();
        let exchanges = json!({ "exchanges": platform.state.lock().unwrap().exchanges.clone() });
        (outputs, exchanges)
    }

    fn held(store: &str, exchanges: &Value) {
        let path = dogfood().join(format!("{store}.json"));
        let written = format!("{}\n", serde_json::to_string_pretty(exchanges).unwrap());
        if std::env::var_os("HERALD_WRITE_DOGFOOD").is_some() {
            std::fs::write(&path, &written).unwrap();
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            committed == written,
            "{} is not what herald sends; run HERALD_WRITE_DOGFOOD=1 cargo nextest run -p herald dogfood and review the diff",
            path.display()
        );
    }

    #[test]
    fn a_release_kept_in_its_post_is_announced_once_against_the_fixtures() {
        let (outputs, exchanges) = announced("markers");
        assert_eq!(
            outputs,
            [
                "releases: announced (post, crosspost, thread, pin, unpin)\n",
                "releases: already announced; nothing sent\n"
            ]
            .map(|line| line.replace("releases", "herald-releases"))
        );
        held("markers", &exchanges);
    }

    #[test]
    fn a_release_kept_in_the_actions_cache_is_announced_once_against_the_fixtures() {
        let (outputs, exchanges) = announced("actions-cache");
        assert_eq!(
            outputs,
            [
                "herald-releases: announced (post, crosspost, thread, pin, unpin)\n",
                "herald-releases: already announced; nothing sent\n"
            ]
        );
        held("actions-cache", &exchanges);
    }
}
