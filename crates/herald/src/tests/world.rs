use crate::*;
use herald_core::{Identity, Key, RouteId};
use herald_ports::{HttpError, Method, Request, Response, Secret};
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
        match (method, file) {
            (Method::Get, Some(file)) => match self.files.get(&file) {
                Some((sha, content)) if self.branch => {
                    answer(200, &json!({ "sha": sha, "content": content }))
                }
                _ => {
                    let inside: Vec<Value> = self
                        .files
                        .keys()
                        .filter(|path| path.starts_with(&format!("{file}/")))
                        .map(|path| json!({ "path": path }))
                        .collect();
                    if inside.is_empty() {
                        answer(404, &json!({}))
                    } else {
                        answer(200, &Value::Array(inside))
                    }
                }
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
channel = "100"
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
    let connect = |base: &str| {
        if base.contains("unreachable") {
            Err(format!("{base} cannot be reached"))
        } else {
            Ok(Connection::new(world.clone()))
        }
    };
    let code = pollster::block_on(crate::run(&all, &setup.vars, &connect, &mut out, &mut err));
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
    .store(Connection::new(world.clone()));
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
