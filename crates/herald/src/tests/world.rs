use crate::*;
use herald_core::{Chore, Identity, Key, RouteId, Schedule};
use herald_ports::{HttpError, Method, Request, Response, Schedules, Secret};
use herald_store_git_branch::GitBranch;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub(super) const NOON: &str = "2026-10-31T12:00:00Z";

pub(super) const EVENT: &str =
    include_str!("../../../herald-sources-forge/fixtures/github/release-event.reference.json");

#[derive(Debug, Default)]
pub(super) struct State {
    pub(super) branch: bool,
    pub(super) files: BTreeMap<String, (String, String)>,
    pub(super) made: u64,
    pub(super) discord: Vec<(String, u16, String)>,
    pub(super) seen: Vec<String>,
    pub(super) failing: Option<Method>,
    pub(super) forge: BTreeMap<String, (u16, String)>,
    pub(super) cache: BTreeMap<String, Vec<u8>>,
    pub(super) variables: BTreeMap<String, String>,
    pub(super) items: BTreeMap<(String, String), Value>,
    pub(super) invocations: std::collections::VecDeque<Value>,
    pub(super) invoked: u32,
    pub(super) given: Vec<(String, Value)>,
    pub(super) handing: Option<u16>,
    pub(super) runtime: Option<u16>,
    pub(super) unheard: bool,
    pub(super) unanswered: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct World {
    pub(super) state: Arc<Mutex<State>>,
}

pub(super) fn answer(status: u16, body: &Value) -> Response {
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
    fn sha(&mut self) -> String {
        self.made += 1;
        format!("sha{}", self.made)
    }

    fn github(&mut self, method: Method, path: &str, body: &Value) -> Response {
        let file = path
            .strip_prefix("/repos/cli/cli/contents/")
            .map(|file| file.trim_end_matches("?ref=herald-state").to_owned());
        if self.failing == Some(method) {
            return answer(500, &json!({}));
        }
        if let Some((status, body)) = self.forge.get(path) {
            return answer(*status, &serde_json::from_str(body).unwrap_or(Value::Null));
        }
        if let (Method::Get, Some(directory)) = (
            method,
            path.strip_prefix("/repos/cli/cli/git/trees/herald-state:"),
        ) {
            let inside: Vec<Value> = self
                .files
                .keys()
                .filter_map(|path| path.strip_prefix(&format!("{directory}/")))
                .map(|name| json!({ "path": name, "type": "blob" }))
                .collect();
            return if inside.is_empty() {
                answer(404, &json!({}))
            } else {
                answer(200, &json!({ "tree": inside, "truncated": false }))
            };
        }
        match (method, file) {
            (Method::Get, Some(file)) => match self.files.get(&file) {
                Some((sha, content)) if self.branch => {
                    answer(200, &json!({ "sha": sha, "content": content }))
                }
                _ => answer(404, &json!({})),
            },
            (Method::Delete, Some(file)) => {
                self.files.remove(&file);
                answer(200, &json!({}))
            }
            (Method::Put, Some(_)) if !self.branch => answer(404, &json!({})),
            (Method::Put, Some(file)) => {
                let sha = self.sha();
                let content = body["content"].as_str().unwrap_or_default().to_owned();
                self.files.insert(file, (sha, content));
                answer(201, &json!({}))
            }
            (Method::Post, None) => {
                self.branch |= path.ends_with("/git/refs");
                let sha = self.sha();
                answer(201, &json!({ "sha": sha }))
            }
            _ => answer(400, &json!({})),
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
                        "signed_download_url": format!("https://blobs.example/{latest}?sig=r"),
                        "matched_key": latest,
                    }),
                ),
                None => answer(200, &json!({ "ok": false })),
            },
            "CreateCacheEntry" if self.cache.contains_key(&key) => answer(409, &json!({})),
            "CreateCacheEntry" => answer(
                200,
                &json!({ "ok": true, "signed_upload_url": format!("https://blobs.example/{key}?sig=w") }),
            ),
            _ => answer(200, &json!({ "ok": true })),
        }
    }

    fn variable(&mut self, method: Method, name: &str, body: &Value) -> Response {
        let name = name.trim_start_matches('/').to_owned();
        match method {
            Method::Get => match self.variables.get(&name) {
                Some(value) => answer(200, &json!({ "name": name, "value": value })),
                None => answer(404, &json!({})),
            },
            Method::Post => {
                let name = body["name"].as_str().unwrap_or_default().to_owned();
                if self.variables.contains_key(&name) {
                    return answer(409, &json!({}));
                }
                let value = body["value"].as_str().unwrap_or_default().to_owned();
                self.variables.insert(name, value);
                answer(201, &Value::Null)
            }
            Method::Patch => {
                let value = body["value"].as_str().unwrap_or_default().to_owned();
                self.variables.insert(name, value);
                answer(204, &Value::Null)
            }
            _ => {
                self.variables.remove(&name);
                answer(204, &Value::Null)
            }
        }
    }

    fn dynamodb(&mut self, target: &str, body: &Value) -> Response {
        let at = |key: &str| {
            let s = |attribute: &str| {
                body.pointer(&format!("/{key}/{attribute}/S"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            (s("directory"), s("name"))
        };
        let version =
            |item: Option<&Value>| item.and_then(|item| item.pointer("/version/N")).cloned();
        let holds =
            |current: Option<&Value>| match body.get("ConditionExpression").and_then(Value::as_str)
            {
                Some("attribute_not_exists(#n)") => current.is_none(),
                _ => version(current).as_ref() == body.pointer("/ExpressionAttributeValues/:v/N"),
            };
        let refused = || {
            answer(
                400,
                &json!({ "__type": "x#ConditionalCheckFailedException" }),
            )
        };
        match target
            .strip_prefix("DynamoDB_20120810.")
            .unwrap_or_default()
        {
            "GetItem" => match self.items.get(&at("Key")) {
                Some(item) => answer(200, &json!({ "Item": item })),
                None => answer(200, &json!({})),
            },
            "Query" => {
                let directory = body.pointer("/ExpressionAttributeValues/:d/S");
                let items: Vec<Value> = self
                    .items
                    .keys()
                    .filter(|(d, _)| Some(&json!(d)) == directory)
                    .map(|(_, name)| json!({ "name": { "S": name } }))
                    .collect();
                answer(200, &json!({ "Items": items }))
            }
            "UpdateItem" => {
                self.made += 1;
                answer(
                    200,
                    &json!({ "Attributes": { "made": { "N": self.made.to_string() } } }),
                )
            }
            "PutItem" if holds(self.items.get(&at("Item"))) => {
                self.items.insert(at("Item"), body["Item"].clone());
                answer(200, &json!({}))
            }
            "DeleteItem" if holds(self.items.get(&at("Key"))) => {
                self.items.remove(&at("Key"));
                answer(200, &json!({}))
            }
            _ => refused(),
        }
    }

    fn discord(&mut self, line: &str) -> Response {
        if self.discord.is_empty() {
            return answer(500, &json!({}));
        }
        let (expected, status, body) = self.discord.remove(0);
        assert!(line.ends_with(&expected), "{line} is not {expected}");
        answer(status, &serde_json::from_str(&body).unwrap_or(Value::Null))
    }
}

impl Http for World {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        let mut state = self.state.lock().unwrap();
        let url = request.url.expose();
        let line = format!("{} {}", request.method.as_str(), url);
        state.seen.push(line.clone());
        if state
            .unanswered
            .iter()
            .any(|path| url.ends_with(path.as_str()))
        {
            return Err(HttpError::TimedOut);
        }
        let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
        if let Some(method) = url.strip_prefix(
            "https://results.example/twirp/github.actions.results.api.v1.CacheService/",
        ) {
            return Ok(state.results(method, &body));
        }
        if let Some(key) = url.strip_prefix("https://blobs.example/") {
            let key = key.split('?').next().unwrap_or_default().to_owned();
            if request.method == Method::Put {
                state.cache.insert(key, request.body);
                return Ok(answer(201, &Value::Null));
            }
            let held = state.cache.get(&key).cloned().unwrap_or_default();
            return Ok(Response {
                status: 200,
                headers: Vec::new(),
                body: held,
            });
        }
        if let Some(rest) = url.strip_prefix("http://127.0.0.1:9001/2018-06-01/runtime/invocation/")
        {
            if rest == "next" {
                if let Some(status) = state.runtime.take() {
                    return Ok(answer(status, &Value::Null));
                }
                let Some(event) = state.invocations.pop_front() else {
                    return Err(HttpError::TimedOut);
                };
                state.invoked += 1;
                return Ok(Response {
                    status: 200,
                    headers: vec![(
                        "Lambda-Runtime-Aws-Request-Id".to_owned(),
                        format!("request-{}", state.invoked),
                    )],
                    body: event.to_string().into_bytes(),
                });
            }
            if state.unheard {
                return Err(HttpError::TimedOut);
            }
            state.given.push((rest.to_owned(), body));
            return Ok(answer(202, &Value::Null));
        }
        if url == "https://lambda.eu-west-1.amazonaws.com/2015-03-31/functions/herald/invocations" {
            if let Some(status) = state.handing {
                return Ok(answer(status, &Value::Null));
            }
            state.invocations.push_back(body);
            return Ok(answer(202, &Value::Null));
        }
        if url == "https://dynamodb.example/" {
            let target = request
                .headers
                .iter()
                .find(|(name, _)| name == "X-Amz-Target")
                .map(|(_, value)| value.expose())
                .unwrap_or_default();
            return Ok(state.dynamodb(&target, &body));
        }
        if let Some(name) =
            url.strip_prefix("https://api.github.com/repos/cli/cli/actions/variables")
        {
            return Ok(state.variable(request.method, name, &body));
        }
        for base in ["https://gitlab.com/api/v4", "https://codeberg.org/api/v1"] {
            if let Some(path) = url.strip_prefix(base) {
                return Ok(match state.forge.get(path) {
                    Some((status, body)) => answer(*status, &serde_json::from_str(body).unwrap()),
                    None => answer(404, &json!({})),
                });
            }
        }
        if let Some(path) = url.strip_prefix("https://api.github.com") {
            return Ok(state.github(request.method, path, &body));
        }
        Ok(state.discord(&line))
    }
}

pub(super) struct Run {
    pub(super) code: u8,
    pub(super) out: String,
    pub(super) err: String,
}

pub(super) const CONFIG: &str = r#"version = 1
[store]
kind = "git-branch"
key = { secret = "HERALD_STORE_KEY" }
[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }
[sources.cli]
kind = "github"
repository = "cli/cli"
[[routes]]
id = "releases"
sources = ["cli"]
destination = "discord"
channel = "1566200000000000100"
template = "release"
[templates.release]
title = "📦 {{ event.project }} {{ event.version }}"
file = "body.md"
"#;

pub(super) struct Setup {
    pub(super) directory: tempfile::TempDir,
    pub(super) vars: BTreeMap<String, String>,
}

pub(super) fn setup(config: &str) -> Setup {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("herald.toml"), config).unwrap();
    std::fs::write(directory.path().join("body.md"), "{{ event.body }}").unwrap();
    std::fs::write(directory.path().join("event.json"), EVENT).unwrap();
    let vars = [
        ("GITHUB_EVENT_NAME", "release".to_owned()),
        (
            "GITHUB_EVENT_PATH",
            directory.path().join("event.json").display().to_string(),
        ),
        ("GITHUB_REPOSITORY", "cli/cli".to_owned()),
        ("GITHUB_TOKEN", "github-token".to_owned()),
        ("GITHUB_RUN_ID", "7".to_owned()),
        ("GITHUB_RUN_ATTEMPT", "1".to_owned()),
        ("HERALD_DISCORD_TOKEN", "discord-token".to_owned()),
        ("HERALD_STORE_KEY", "store-key".to_owned()),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect();
    Setup { directory, vars }
}

pub(super) fn herald(setup: &Setup, world: &World, args: &[&str]) -> Run {
    let config = setup
        .directory
        .path()
        .join("herald.toml")
        .display()
        .to_string();
    let mut all: Vec<String> = vec!["herald".to_owned(), "--config".to_owned(), config];
    all.extend(args.iter().map(|arg| (*arg).to_owned()));
    let mut out = Vec::new();
    let mut err = Vec::new();
    let connect = |base: &str, _: &herald_ports::Outbound| {
        if base.contains("unreachable") {
            Err(format!("{base} cannot be reached"))
        } else {
            Ok(Connection::new(world.clone()))
        }
    };
    let code = on_tokio(crate::run(
        &all,
        &setup.vars,
        &Connector::new(&connect, &connect),
        &mut out,
        &mut err,
    ));
    Run {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

/// What `herald` with `args` does in `world`, a server it runs stopped once
/// `stop` completes.
pub(super) fn herald_until(
    setup: &Setup,
    world: &World,
    args: &[&str],
    stop: impl std::future::Future<Output = ()>,
) -> Run {
    let config = setup
        .directory
        .path()
        .join("herald.toml")
        .display()
        .to_string();
    let mut all: Vec<String> = vec!["herald".to_owned(), "--config".to_owned(), config];
    all.extend(args.iter().map(|arg| (*arg).to_owned()));
    let mut out = Vec::new();
    let mut err = Vec::new();
    let connect = |_: &str, _: &herald_ports::Outbound| Ok(Connection::new(world.clone()));
    let code = on_tokio(crate::run_until(
        &all,
        &setup.vars,
        &Connector::new(&connect, &connect),
        &mut out,
        &mut err,
        stop,
    ));
    Run {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

pub(super) const ANNOUNCE: [&str; 3] = ["announce", "--from", "github"];

pub(super) const MARKERS: &str = "kind = \"markers\"";

pub(super) const OVERDUE: &str = "2020-01-01T00:00:00Z";

pub(super) const DISTANT: &str = "2100-01-01T00:00:00Z";

pub(super) fn scheduled(world: &World, schedules: &[(&str, &str, Chore, &str)]) {
    let store = GitBranch::new(
        "cli/cli",
        Secret::new("GITHUB_TOKEN", "github-token"),
        None,
        None,
    )
    .store(Connection::new(world.clone()))
    .sealed(herald_records::Seal::new(Secret::new(
        "HERALD_STORE_KEY",
        "store-key",
    )));
    let schedules: Vec<Schedule> = schedules
        .iter()
        .map(|&(route, thread, chore, due)| Schedule {
            key: Key::derive(&RouteId::new(route), &Identity::new(thread)),
            route: RouteId::new(route),
            thread: thread.to_owned(),
            chore,
            due: due.parse().unwrap(),
        })
        .collect();
    pollster::block_on(store.put_schedules(&schedules, &[])).unwrap();
}

pub(super) fn discord(world: &World, answers: &[(&str, u16, &str)]) {
    world.state.lock().unwrap().discord.extend(
        answers
            .iter()
            .map(|&(path, status, body)| (path.to_owned(), status, body.to_owned())),
    );
}

pub(super) const RELEASES: &str = "/repos/cli/cli/releases?per_page=30";

pub(super) const REPOSITORY: &str = "/repos/cli/cli";

pub(super) fn forge(world: &World, releases: (u16, &[&str])) {
    let (status, versions) = releases;
    let listed: Vec<Value> = versions
        .iter()
        .map(|version| {
            json!({
                "tag_name": version,
                "name": format!("GitHub CLI {version}"),
                "body": "## What's Changed",
                "draft": false,
                "prerelease": false,
                "published_at": "2026-10-31T12:00:00Z",
                "html_url": format!("https://github.com/cli/cli/releases/tag/{version}"),
            })
        })
        .collect();
    let mut state = world.state.lock().unwrap();
    state.forge.insert(
        RELEASES.to_owned(),
        (status, Value::Array(listed).to_string()),
    );
    state.forge.insert(
        REPOSITORY.to_owned(),
        (200, r#"{"id":212613049,"name":"cli"}"#.to_owned()),
    );
}

pub(super) const POLL: [&str; 1] = ["poll"];

pub(super) fn with_release(world: &World, status: u16, draft: bool) {
    let release = json!({
        "tag_name": "v1",
        "name": "GitHub CLI v1",
        "body": "## What's Changed",
        "draft": draft,
        "prerelease": false,
        "published_at": "2026-10-31T12:00:00Z",
        "html_url": "https://github.com/cli/cli/releases/tag/v1",
    });
    forge(world, (200, &[]));
    world.state.lock().unwrap().forge.insert(
        "/repos/cli/cli/releases/tags/v1".to_owned(),
        (status, release.to_string()),
    );
}

/// `future` run to its end on a runtime like the binary's own, so what it
/// spawns, such as a database connection, runs too.
pub(super) fn on_tokio<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}
