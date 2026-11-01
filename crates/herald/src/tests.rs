#![allow(
    clippy::unused_async_trait_impl,
    reason = "the model answers at once, and is written as the async trait it fakes"
)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use herald_core::{Identity, Key, RouteId};
use herald_ports::{HttpError, Method, Request, Response, Secret};
use serde_json::{Value, json};

use super::*;

const NOON: &str = "2026-10-31T12:00:00Z";

const EVENT: &str =
    include_str!("../../herald-sources-forge/fixtures/github/release-event.reference.json");

#[derive(Debug, Default)]
struct State {
    branch: bool,
    files: BTreeMap<String, (String, String)>,
    made: u64,
    discord: Vec<(String, u16, String)>,
    seen: Vec<String>,
    failing: Option<Method>,
    forge: BTreeMap<String, (u16, String)>,
    cache: BTreeMap<String, Vec<u8>>,
    variables: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
struct World {
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
        if let Some(path) = url.strip_prefix("https://api.github.com") {
            return Ok(state.github(request.method, path, &body));
        }
        Ok(state.discord(&line))
    }
}

struct Run {
    code: u8,
    out: String,
    err: String,
}

const CONFIG: &str = r#"version = 1
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

struct Setup {
    directory: tempfile::TempDir,
    vars: BTreeMap<String, String>,
}

fn setup(config: &str) -> Setup {
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

fn run(setup: &Setup, world: &World, args: &[&str]) -> Run {
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
    let code = pollster::block_on(super::run(&all, &setup.vars, &connect, &mut out, &mut err));
    Run {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

const ANNOUNCE: [&str; 3] = ["announce", "--from", "github"];

#[test]
fn a_published_release_is_announced_once_and_kept_on_the_state_branch() {
    let setup = setup(CONFIG);
    let world = World::default();
    world.state.lock().unwrap().discord.push((
        "/channels/100/messages".to_owned(),
        200,
        r#"{"id":"1"}"#.to_owned(),
    ));
    let first = run(&setup, &world, &ANNOUNCE);
    assert_eq!(
        (first.code, first.out.as_str(), first.err.as_str()),
        (DONE, "releases: announced (post)\n", "")
    );
    assert!(world.state.lock().unwrap().branch);
    let again = run(&setup, &world, &ANNOUNCE);
    assert_eq!(again.out, "releases: already announced; nothing sent\n");
}

#[test]
fn a_refusal_stops_the_run_and_says_what_remains() {
    let setup = setup(CONFIG);
    let world = World::default();
    world.state.lock().unwrap().discord.push((
        "/channels/100/messages".to_owned(),
        403,
        r#"{"code":50013,"message":"Missing Permissions"}"#.to_owned(),
    ));
    let stopped = run(&setup, &world, &ANNOUNCE);
    assert_eq!(stopped.code, STOPPED);
    assert_eq!(
        stopped.out,
        "releases: stopped: Discord refused it with 403: 50013 Missing Permissions. Landed: nothing. The next run sends what remains: post.\n"
    );
}

#[test]
fn an_event_that_is_not_a_published_release_announces_nothing() {
    let mut setup = setup(CONFIG);
    let world = World::default();
    setup
        .vars
        .insert("GITHUB_EVENT_NAME".to_owned(), "push".to_owned());
    let push = run(&setup, &world, &ANNOUNCE);
    assert_eq!(
        (push.code, push.out.as_str()),
        (
            DONE,
            "The run's event is push, not a release; nothing to announce.\n"
        )
    );
    setup
        .vars
        .insert("GITHUB_EVENT_NAME".to_owned(), "release".to_owned());
    std::fs::write(
        setup.directory.path().join("event.json"),
        EVENT.replace("\"published\"", "\"created\""),
    )
    .unwrap();
    let created = run(&setup, &world, &ANNOUNCE);
    assert_eq!(
        created.out,
        "The release is not published, or is a draft; nothing to announce.\n"
    );
    std::fs::write(setup.directory.path().join("event.json"), "{}").unwrap();
    let garbled = run(&setup, &world, &ANNOUNCE);
    assert_eq!(garbled.code, STOPPED);
    assert!(
        garbled
            .err
            .starts_with("GitHub's release event could not be read"),
        "{}",
        garbled.err
    );
}

#[test]
fn a_run_that_cannot_start_says_why() {
    let world = World::default();
    let cases: [(&str, Option<&str>, &str, &str); 7] = [
        (
            "GITHUB_EVENT_NAME",
            None,
            CONFIG,
            "GITHUB_EVENT_NAME is not set; this command runs in GitHub Actions",
        ),
        (
            "GITHUB_EVENT_PATH",
            Some("/nonexistent/event.json"),
            CONFIG,
            "/nonexistent/event.json could not be read:",
        ),
        (
            "GITHUB_REPOSITORY",
            Some("other/repo"),
            CONFIG,
            "no source in the config names other/repo",
        ),
        (
            "GITHUB_TOKEN",
            None,
            CONFIG,
            "GITHUB_TOKEN is not set; the git-branch store needs a token that may push",
        ),
        ("HERALD_DISCORD_TOKEN", None, CONFIG, "HERALD_DISCORD_TOKEN"),
        (
            "GITHUB_RUN_ID",
            None,
            &CONFIG.replace("kind = \"git-branch\"", "kind = \"sqlite\""),
            "this herald keeps records in git-branch, markers, actions-cache or repo-variables; set [store] kind to one of them",
        ),
        (
            "GITHUB_RUN_ID",
            None,
            &CONFIG.replace(
                "kind = \"git-branch\"",
                "kind = \"git-branch\"\napi = \"https://unreachable.example\"",
            ),
            "https://unreachable.example cannot be reached",
        ),
    ];
    for (name, value, config, message) in cases {
        let mut setup = setup(config);
        match value {
            Some(value) => setup.vars.insert(name.to_owned(), value.to_owned()),
            None => setup.vars.remove(name),
        };
        let failed = run(&setup, &world, &ANNOUNCE);
        assert_eq!(failed.code, STOPPED, "{name}: {}", failed.err);
        assert!(failed.err.contains(message), "{name}: {}", failed.err);
    }
}

#[test]
fn a_destination_that_cannot_be_reached_or_a_run_without_ids_still_answers() {
    let config = CONFIG.replace(
        "token = { secret = \"HERALD_DISCORD_TOKEN\" }",
        "token = { secret = \"HERALD_DISCORD_TOKEN\" }\napi = \"https://unreachable.example/api\"",
    );
    let failed = run(&setup(&config), &World::default(), &ANNOUNCE);
    assert!(
        failed
            .err
            .contains("https://unreachable.example/api cannot be reached"),
        "{}",
        failed.err
    );
    let mut local = setup(CONFIG);
    local.vars.remove("GITHUB_RUN_ID");
    let world = World::default();
    world.state.lock().unwrap().discord.push((
        "/channels/100/messages".to_owned(),
        200,
        r#"{"id":"1"}"#.to_owned(),
    ));
    assert_eq!(run(&local, &world, &ANNOUNCE).code, DONE);
}

#[test]
fn doctor_names_what_the_config_leaves_to_chance() {
    let world = World::default();
    let derived = run(&setup(CONFIG), &world, &["doctor", "--layer", "actions"]);
    assert_eq!(derived.code, DONE);
    assert!(
        derived
            .out
            .contains("reads, its names resolve and its templates render.")
    );
    assert!(
        derived
            .out
            .contains("Rotating that credential forgets every earlier marker")
    );
    let keyed = run(
        &setup(&format!(
            "{CONFIG}[markers]\nkey = {{ secret = \"HERALD_MARKER_SECRET\" }}\n"
        )),
        &world,
        &["doctor"],
    );
    assert!(!keyed.out.contains("Rotating"), "{}", keyed.out);
    let sqlite = run(
        &setup(&CONFIG.replace("kind = \"git-branch\"", "kind = \"sqlite\"")),
        &world,
        &["doctor"],
    );
    assert_eq!(sqlite.code, STOPPED);
    assert!(
        sqlite.out.ends_with("store.kind: keeping records in sqlite cannot be served: this herald keeps records in git-branch, markers, actions-cache or repo-variables; set [store] kind to one of them.\n"),
        "{}",
        sqlite.out
    );
}

const MARKERS: &str = "kind = \"markers\"";

#[test]
fn doctor_names_what_the_markers_store_cannot_serve() {
    let gaps = |config: &str| {
        let doctored = run(&setup(config), &World::default(), &["doctor"]);
        assert_eq!(doctored.code, STOPPED, "{}", doctored.err);
        assert!(
            doctored
                .out
                .contains("The markers store has no compare-and-set")
        );
        doctored
            .out
            .lines()
            .filter(|line| line.contains("cannot be served"))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    };
    let markers = CONFIG.replace("kind = \"git-branch\"", MARKERS);
    let polling = "sources.cli: polling cli cannot be served: the markers store keeps no cursors; `herald poll` needs a store that does.";
    assert_eq!(
        gaps(&markers.replace(
            "template = \"release\"\n",
            "template = \"release\"\npin = \"store\"\n"
        )),
        [
            "routes[0].pin: a pin the store remembers cannot be served: the markers store keeps no pins; set pin = \"markers\".",
            polling,
        ]
    );
    let hooked = markers.replace(
        "token = { secret = \"HERALD_DISCORD_TOKEN\" }",
        "webhook = { secret = \"HERALD_DISCORD_WEBHOOK\" }",
    );
    assert_eq!(
        gaps(&hooked),
        [
            "routes[0].destination: sending each announcement once cannot be served: the markers store reads announcements back from their posts, and a webhook cannot read back its posts; post through a bot to a text or announcement channel, or choose a store that keeps records.",
            polling,
        ]
    );
    let forum = markers.replace("channel = \"100\"", "channel = \"100\"\nforum = true");
    assert!(gaps(&forum)[0].contains("and a forum's posts are not read back"));
}

#[test]
fn a_release_kept_only_in_its_post_is_announced_once_and_each_command_warns_of_its_gaps() {
    let mut setup = setup(&CONFIG.replace("kind = \"git-branch\"", MARKERS));
    setup.vars.remove("GITHUB_TOKEN");
    let world = World::default();
    discord(
        &world,
        &[
            ("/users/@me", 200, r#"{"id":"99"}"#),
            ("/channels/100/messages?limit=100", 200, "[]"),
            ("/channels/100/messages", 200, r#"{"id":"1"}"#),
        ],
    );
    let announced = run(&setup, &world, &ANNOUNCE);
    assert_eq!(
        (announced.code, announced.out.as_str()),
        (
            DONE,
            "warning: sources.cli: polling cli cannot be served: the markers store keeps no cursors; `herald poll` needs a store that does.\nreleases: announced (post)\n"
        )
    );
    setup
        .vars
        .insert("GITHUB_ACTIONS".to_owned(), "true".to_owned());
    let polled = run(&setup, &world, &POLL);
    assert_eq!(
        (polled.code, polled.out.as_str()),
        (
            STOPPED,
            "::warning title=herald doctor::sources.cli: polling cli cannot be served: the markers store keeps no cursors; `herald poll` needs a store that does.\ncli: stopped: this store keeps no cursors; choose a store that does. The next poll asks again.\n"
        )
    );
    let housekept = run(&setup, &world, &["housekeep"]);
    assert!(
        housekept.out.ends_with("No chore is due.\n"),
        "{}",
        housekept.out
    );
}

#[test]
fn a_config_that_cannot_be_read_or_has_mistakes_is_reported() {
    let world = World::default();
    let mistaken = run(&setup("version = 2\n"), &world, &["doctor"]);
    assert_eq!(mistaken.code, MISTAKEN);
    assert!(
        mistaken
            .err
            .contains("version: this herald reads version 1"),
        "{}",
        mistaken.err
    );
    let args = [
        "herald".to_owned(),
        "--config".to_owned(),
        "/nonexistent/herald.toml".to_owned(),
        "doctor".to_owned(),
    ];
    let mut err = Vec::new();
    let missing = pollster::block_on(super::run(
        &args,
        &BTreeMap::new(),
        &|_| Ok(Connection::new(world.clone())),
        &mut Vec::new(),
        &mut err,
    ));
    assert_eq!(missing, MISTAKEN);
    assert!(
        String::from_utf8(err)
            .unwrap()
            .starts_with("/nonexistent/herald.toml could not be read")
    );
}

#[test]
fn a_template_file_that_cannot_be_read_and_a_store_token_of_its_own_are_named() {
    let world = World::default();
    let missing = run(
        &setup(&CONFIG.replace("body.md", "missing.md")),
        &world,
        &["doctor"],
    );
    assert_eq!(missing.code, MISTAKEN);
    assert!(
        missing.err.contains("missing.md could not be read"),
        "{}",
        missing.err
    );
    let tokened = CONFIG.replace(
        "kind = \"git-branch\"",
        "kind = \"git-branch\"\ntoken = { secret = \"STATE_TOKEN\" }",
    );
    let unset = run(&setup(&tokened), &world, &ANNOUNCE);
    assert!(
        unset.err.contains("STATE_TOKEN is not set"),
        "{}",
        unset.err
    );
}

#[test]
fn usage_mistakes_and_help_answer_as_clap_does() {
    let world = World::default();
    let setup = setup(CONFIG);
    let unknown = run(&setup, &world, &["fly"]);
    assert_eq!(unknown.code, 2);
    assert!(
        unknown.err.contains("unrecognized subcommand"),
        "{}",
        unknown.err
    );
    let help = run(&setup, &world, &["--help"]);
    assert_eq!(help.code, 0);
    assert!(help.out.contains("Announce what happened"), "{}", help.out);
}

#[test]
fn every_outcome_and_step_reads_as_a_line() {
    let announced = |outcome| Announced {
        route: RouteId::new("r"),
        outcome,
    };
    let until: jiff::Timestamp = NOON.parse().unwrap();
    assert_eq!(
        report(&announced(Outcome::Held {
            by: "github/1/1".to_owned(),
            until
        })),
        "r: run github/1/1 holds it until 2026-10-31T12:00:00Z; nothing sent by this run"
    );
    assert_eq!(
        report(&announced(Outcome::Stopped {
            landed: vec![Step::Post, Step::Chunk(1)],
            remaining: Vec::new(),
            reason: Stop::RetryAfter(jiff::SignedDuration::from_secs(90)),
        })),
        "r: stopped: the destination asked to wait 1m 30s. Landed: post, chunk 1. The next run sends what remains: nothing."
    );
    for reason in [
        Stop::Unreachable("u".to_owned()),
        Stop::Store("s".to_owned()),
        Stop::Template("t".to_owned()),
        Stop::Marker("m".to_owned()),
    ] {
        let line = report(&announced(Outcome::Stopped {
            landed: Vec::new(),
            remaining: vec![Step::Post],
            reason,
        }));
        assert!(line.starts_with("r: stopped: "), "{line}");
    }
    let all = [
        Step::Post,
        Step::Crosspost,
        Step::OpenThread,
        Step::Chunk(2),
        Step::Pin,
        Step::Unpin,
        Step::Deliver("https://a.example/inbox".to_owned()),
    ];
    assert_eq!(
        steps(&all),
        "post, crosspost, thread, chunk 2, pin, unpin, delivery to https://a.example/inbox"
    );
}

const OVERDUE: &str = "2020-01-01T00:00:00Z";
const DISTANT: &str = "2100-01-01T00:00:00Z";

fn scheduled(world: &World, schedules: &[(&str, &str, Chore, &str)]) {
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

fn discord(world: &World, answers: &[(&str, u16, &str)]) {
    world.state.lock().unwrap().discord.extend(
        answers
            .iter()
            .map(|&(path, status, body)| (path.to_owned(), status, body.to_owned())),
    );
}

#[test]
fn housekeeping_does_each_chore_that_fell_due_and_says_so() {
    let setup = setup(CONFIG);
    let world = World::default();
    let idle = run(&setup, &world, &["housekeep"]);
    assert_eq!(
        (idle.code, idle.out.as_str(), idle.err.as_str()),
        (DONE, "No chore is due.\n", "")
    );
    scheduled(
        &world,
        &[
            ("releases", "7", Chore::Archive, OVERDUE),
            ("gone", "8", Chore::Lock, OVERDUE),
            ("releases", "9", Chore::Lock, DISTANT),
        ],
    );
    discord(&world, &[("/channels/7", 200, "{}")]);
    let kept = run(&setup, &world, &["housekeep"]);
    assert_eq!(
        (kept.code, kept.out.as_str()),
        (
            DONE,
            "releases: thread 7 archived\ngone: thread 8 not locked, and not tried again: the route is no longer in the config\n"
        )
    );
    let again = run(&setup, &world, &["housekeep"]);
    assert_eq!(again.out, "No chore is due.\n");
}

#[test]
fn housekeeping_that_must_wait_or_cannot_read_its_store_stops() {
    let setup = setup(CONFIG);
    let world = World::default();
    scheduled(
        &world,
        &[
            ("releases", "7", Chore::Archive, OVERDUE),
            ("releases", "8", Chore::Archive, OVERDUE),
        ],
    );
    discord(&world, &[("/channels/7", 429, r#"{"retry_after":2}"#)]);
    let waiting = run(&setup, &world, &["housekeep"]);
    assert_eq!(waiting.code, STOPPED);
    assert_eq!(
        waiting.out.lines().collect::<Vec<_>>(),
        [
            "releases: thread 7 not archived yet: the destination asked to wait 2s. The next run tries again.",
            "releases: thread 8 not archived yet: the destination asked to wait 2s. The next run tries again.",
        ]
    );
    world.state.lock().unwrap().failing = Some(Method::Get);
    let unread = run(&setup, &world, &["housekeep"]);
    assert_eq!(
        (unread.code, unread.out.as_str(), unread.err.as_str()),
        (
            STOPPED,
            "",
            "the store could not be reached: GitHub answered 500\n"
        )
    );
    let mut outside = setup;
    outside.vars.remove("GITHUB_REPOSITORY");
    let unset = run(&outside, &world, &["housekeep"]);
    assert_eq!(
        unset.err,
        "GITHUB_REPOSITORY is not set; this command runs in GitHub Actions\n"
    );
}

const RELEASES: &str = "/repos/cli/cli/releases?per_page=30";
const REPOSITORY: &str = "/repos/cli/cli";

fn forge(world: &World, releases: (u16, &[&str])) {
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

const POLL: [&str; 1] = ["poll"];

#[test]
fn a_poll_remembers_what_it_first_finds_and_announces_what_is_new() {
    let setup = setup(CONFIG);
    let world = World::default();
    forge(&world, (200, &["v1"]));
    let first = run(&setup, &world, &POLL);
    assert_eq!(
        (first.code, first.out.as_str(), first.err.as_str()),
        (
            DONE,
            "cli: first poll found 1; 0 announced, as backfill asks\n",
            ""
        )
    );
    forge(&world, (200, &["v2", "v1"]));
    discord(&world, &[("/channels/100/messages", 200, r#"{"id":"1"}"#)]);
    let later = run(&setup, &world, &POLL);
    assert_eq!(
        (later.code, later.out.as_str()),
        (
            DONE,
            "cli: 2 found, 1 new\ncli v2: releases: announced (post)\n"
        )
    );
    forge(&world, (304, &[]));
    let unchanged = run(&setup, &world, &POLL);
    assert_eq!(unchanged.out, "cli: unchanged since the last poll\n");
}

#[test]
fn a_poll_that_stops_says_what_the_next_poll_does() {
    let setup = setup(CONFIG);
    let world = World::default();
    forge(&world, (200, &["v1"]));
    world.state.lock().unwrap().failing = Some(Method::Put);
    let unkept = run(&setup, &world, &POLL);
    assert_eq!(
        (unkept.code, unkept.out.as_str()),
        (
            STOPPED,
            "cli: first poll found 1; 0 announced, as backfill asks; the cursor stays where it was: the store could not be reached: GitHub answered 500. The next poll offers them again\n"
        )
    );
    world.state.lock().unwrap().failing = None;
    assert_eq!(run(&setup, &world, &POLL).code, DONE);
    forge(&world, (200, &["v2", "v1"]));
    discord(
        &world,
        &[(
            "/channels/100/messages",
            403,
            r#"{"code":50013,"message":"Missing Permissions"}"#,
        )],
    );
    let refused = run(&setup, &world, &POLL);
    assert_eq!(refused.code, STOPPED);
    assert!(
        refused
            .out
            .starts_with("cli: 2 found, 1 new; a route stopped, so the next poll offers them again\ncli v2: releases: stopped: Discord refused it with 403"),
        "{}",
        refused.out
    );
    world
        .state
        .lock()
        .unwrap()
        .forge
        .insert(REPOSITORY.to_owned(), (404, "{}".to_owned()));
    let unread = run(&setup, &world, &POLL);
    assert_eq!(
        (unread.code, unread.out.as_str()),
        (
            STOPPED,
            "cli: stopped: GitHub's repository was answered with 404. The next poll asks again.\n"
        )
    );
}

#[test]
fn a_config_without_sources_has_nothing_to_poll() {
    let sourceless = "version = 1\n[store]\nkind = \"git-branch\"\n[destinations.discord]\nkind = \"discord\"\ntoken = { secret = \"HERALD_DISCORD_TOKEN\" }\n";
    let idle = run(&setup(sourceless), &World::default(), &POLL);
    assert_eq!(
        (idle.code, idle.out.as_str(), idle.err.as_str()),
        (DONE, "The config names no source to poll.\n", "")
    );
}

#[test]
fn a_warning_on_actions_stays_one_workflow_command() {
    assert_eq!(
        escaped("100% done\r\n::error::forged"),
        "100%25 done%0D%0A::error::forged"
    );
    let mut setup = setup(
        &CONFIG
            .replace("kind = \"git-branch\"", MARKERS)
            .replace("[sources.cli]", "[sources.\"cli\\n::error::forged\"]")
            .replace(
                "sources = [\"cli\"]",
                "sources = [\"cli\\n::error::forged\"]",
            ),
    );
    setup
        .vars
        .insert("GITHUB_ACTIONS".to_owned(), "true".to_owned());
    setup.vars.insert(
        "GITHUB_EVENT_NAME".to_owned(),
        "push\n::error::forged".to_owned(),
    );
    let warned = run(&setup, &World::default(), &ANNOUNCE);
    assert_eq!(
        warned
            .out
            .lines()
            .filter(|line| line.starts_with("::"))
            .count(),
        1,
        "{}",
        warned.out
    );
    assert!(
        warned
            .out
            .contains("The run's event is push ::error::forged, not a release"),
        "{}",
        warned.out
    );
}

#[test]
fn a_release_is_announced_once_through_the_actions_cache() {
    let mut setup = setup(&CONFIG.replace("kind = \"git-branch\"", "kind = \"actions-cache\""));
    let world = World::default();
    discord(&world, &[("/channels/100/messages", 200, r#"{"id":"1"}"#)]);
    let unset = run(&setup, &world, &ANNOUNCE);
    assert_eq!(
        unset.err,
        "ACTIONS_RESULTS_URL is not set; this command runs in GitHub Actions\n"
    );
    setup.vars.insert(
        "ACTIONS_RESULTS_URL".to_owned(),
        "https://results.example".to_owned(),
    );
    let tokenless = run(&setup, &world, &ANNOUNCE);
    assert_eq!(
        tokenless.err,
        "ACTIONS_RUNTIME_TOKEN is not set; the actions-cache store runs through herald's Action\n"
    );
    setup
        .vars
        .insert("ACTIONS_RUNTIME_TOKEN".to_owned(), "runtime".to_owned());
    let first = run(&setup, &world, &ANNOUNCE);
    assert_eq!(
        (first.code, first.out.as_str(), first.err.as_str()),
        (DONE, "releases: announced (post)\n", "")
    );
    let again = run(&setup, &world, &ANNOUNCE);
    assert_eq!(again.out, "releases: already announced; nothing sent\n");
}

#[test]
fn a_release_is_announced_once_through_the_repository_s_variables() {
    let config = CONFIG.replace(
        "kind = \"git-branch\"",
        "kind = \"repo-variables\"\ntoken = { secret = \"HERALD_STATE_TOKEN\" }",
    );
    let mut setup = setup(&config);
    let world = World::default();
    discord(&world, &[("/channels/100/messages", 200, r#"{"id":"1"}"#)]);
    let unset = run(&setup, &world, &ANNOUNCE);
    assert_eq!(
        unset.err,
        "HERALD_STATE_TOKEN is not set; the repo-variables store needs a token with the repository's Variables permission\n"
    );
    setup
        .vars
        .insert("HERALD_STATE_TOKEN".to_owned(), "variables".to_owned());
    let first = run(&setup, &world, &ANNOUNCE);
    assert_eq!(
        (first.code, first.out.as_str(), first.err.as_str()),
        (DONE, "releases: announced (post)\n", "")
    );
    let again = run(&setup, &world, &ANNOUNCE);
    assert_eq!(again.out, "releases: already announced; nothing sent\n");
    assert!(
        world
            .state
            .lock()
            .unwrap()
            .variables
            .keys()
            .all(|name| name.starts_with("HERALD_"))
    );
}
