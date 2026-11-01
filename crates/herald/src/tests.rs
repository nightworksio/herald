#![allow(
    clippy::unused_async_trait_impl,
    reason = "the model answers at once, and is written as the async trait it fakes"
)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use herald_core::RouteId;
use herald_ports::{HttpError, Method, Request, Response};
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
        match (method, file) {
            (Method::Get, Some(file)) => match self.files.get(&file) {
                Some((sha, content)) if self.branch => {
                    answer(200, &json!({ "sha": sha, "content": content }))
                }
                _ => answer(404, &json!({})),
            },
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
            "GITHUB_EVENT_NAME is not set; herald announce --from github runs in GitHub Actions",
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
            &CONFIG.replace("kind = \"git-branch\"", "kind = \"markers\""),
            "this herald keeps records only on a git branch",
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
    let derived = run(&setup(CONFIG), &world, &["doctor"]);
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
    let markers = run(
        &setup(&CONFIG.replace("kind = \"git-branch\"", "kind = \"markers\"")),
        &world,
        &["doctor"],
    );
    assert_eq!(markers.code, STOPPED);
    assert!(markers.out.contains("set [store] kind = \"git-branch\""));
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
