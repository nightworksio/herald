//! The sessions a destination holds: Discord's gateway, Slack's Socket Mode
//! and a Matrix sync, each answering its commands and reconnecting.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::future::Future;
use std::pin::{Pin, pin};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::stream::{FuturesUnordered, StreamExt};
use herald_config::{Config, Uses, load};
use herald_core::{DestinationName, Kind, Latest, RouteId, Shown};
use herald_flows::{
    Connected, Did, Job, Ports, Serving, Sessions, Taker, Webhooks, Worked, prepare, work,
};
use herald_ports::{Announcements, Frame, Http, HttpError, Request, Response, Timer};
use herald_records::MemoryKeyring;
use herald_testing::{
    FakeClock, FakeSecrets, FakeSigner, FakeStore, FakeVerifier, ScriptedSockets,
};
use serde_json::{Value, json};
use tokio::sync::Notify;

use crate::harness::keys_in;

const APPLICATION: &str = "123456789012345678";
const CHANNEL: &str = "323456789012345678";
const GUILD: &str = "223456789012345678";
const ROLE: &str = "523456789012345678";
const TOKEN: &str = "«secret:the interaction's token»";
const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// A config whose one destination, `kind`, holds a session: `discord on
/// <os>` is a Discord one identifying as that operating system.
fn config(kind: &str) -> Config {
    let os = kind
        .strip_prefix("discord on ")
        .map_or_else(String::new, |os| format!("\nos = \"{os}\""));
    let destination = match kind {
        _ if kind.starts_with("discord") => format!(
            r#"[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[destinations.discord.interactions]
application = "{APPLICATION}"
via = "gateway"{os}
[[routes]]
id = "discorded"
sources = ["gate"]
destination = "discord"
channel = "{CHANNEL}"
template = "t"
subscribe = {{ role = "{ROLE}" }}"#
        ),
        "slack" => r#"[destinations.slack]
kind = "slack"
token = { secret = "HERALD_SLACK_TOKEN" }
[destinations.slack.interactions]
via = "socket"
app = { secret = "HERALD_SLACK_APP" }
[[routes]]
id = "slacked"
sources = ["gate"]
destination = "slack"
channel = "C0123456789"
template = "t""#
            .to_owned(),
        _ => r#"[destinations.matrix]
kind = "matrix"
homeserver = "https://matrix.example"
token = { secret = "HERALD_MATRIX_TOKEN" }
[destinations.matrix.interactions]
via = "sync"
[[routes]]
id = "roomed"
sources = ["gate"]
destination = "matrix"
room = "!room:matrix.example"
template = "t""#
            .to_owned(),
    };
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[sources.gate]
kind = "github"
repository = "cli/cli"
[templates.t]
title = "{{{{ event.project }}}} {{{{ event.attributes.version }}}}"
body = "{{{{ event.body }}}}"
[destinations.posted]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[destinations.posted.interactions]
application = "{APPLICATION}"
key = "{KEY}"
[destinations.hooked]
kind = "slack"
token = {{ secret = "HERALD_SLACK_TOKEN" }}
[destinations.hooked.interactions]
signing = {{ secret = "HERALD_SLACK_SIGNING" }}
app = {{ secret = "HERALD_SLACK_APP" }}
[destinations.pushed]
kind = "matrix"
homeserver = "https://matrix.example"
token = {{ secret = "HERALD_MATRIX_TOKEN" }}
[destinations.pushed.interactions]
homeserver_token = {{ secret = "HERALD_MATRIX_HS" }}
appservice_token = {{ secret = "HERALD_MATRIX_AS" }}
{destination}
"#
    );
    let mut config = load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap();
    config.server.sessions.jitter = false;
    config
}

fn secrets() -> FakeSecrets {
    FakeSecrets::default()
        .with("HERALD_DISCORD_TOKEN", "bot-token")
        .with("HERALD_SLACK_TOKEN", "slack-token")
        .with("HERALD_SLACK_APP", "app-token")
        .with("HERALD_MATRIX_TOKEN", "matrix-token")
        .with("HERALD_SLACK_SIGNING", "signing-secret")
        .with("HERALD_MATRIX_HS", "hs-token")
        .with("HERALD_MATRIX_AS", "as-token")
}

/// A path a request's URL holds, and the status and body it is answered
/// with, or `None` where it hangs.
type Scripted = (String, Option<(u16, String)>);

/// An `Http` answering each request, after giving the rest of its task a
/// turn, with the first scripted answer whose path its URL holds, or
/// hanging where the answer is `None`, and failing once the script is
/// spent.
#[derive(Debug, Clone, Default)]
struct Answers {
    script: Arc<Mutex<VecDeque<Scripted>>>,
    seen: Arc<Mutex<Vec<(String, tokio::time::Instant)>>>,
}

impl Answers {
    fn of(script: Vec<Scripted>) -> Self {
        Self {
            script: Arc::new(Mutex::new(script.into())),
            seen: Arc::default(),
        }
    }

    fn seen(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|(line, _)| line.clone())
            .collect()
    }

    /// How long after the first request holding `path` each later one
    /// was sent, in whole seconds.
    fn spaced(&self, path: &str) -> Vec<u64> {
        let at: Vec<tokio::time::Instant> = self
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(line, _)| line.contains(path))
            .map(|(_, at)| *at)
            .collect();
        at.windows(2)
            .map(|pair| pair[1].duration_since(pair[0]).as_secs())
            .collect()
    }
}

impl Http for Answers {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        tokio::task::yield_now().await;
        let line = format!("{} {}", request.method.as_str(), request.url.redacted());
        self.seen
            .lock()
            .unwrap()
            .push((line.clone(), tokio::time::Instant::now()));
        let next = {
            let mut script = self.script.lock().unwrap();
            let found = script
                .iter()
                .position(|(expected, _)| line.contains(expected));
            found.and_then(|index| script.remove(index))
        };
        let Some((_, answer)) = next else {
            return Err(HttpError::Unreachable {
                reason: format!("nothing scripted for {line}"),
            });
        };
        let Some((status, body)) = answer else {
            return std::future::pending().await;
        };
        Ok(Response {
            status,
            headers: Vec::new(),
            body: body.into_bytes(),
        })
    }
}

/// A timer that waits as long as it is asked to.
struct Sleeping;

impl Timer for Sleeping {
    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

/// Takes a command's work while fewer than `most` are held, and hears what
/// the sessions said.
struct Taken<'a> {
    most: usize,
    held: Cell<usize>,
    jobs: RefCell<VecDeque<Job>>,
    arrived: Notify,
    hear: &'a dyn Fn(String),
}

impl Taker for Taken<'_> {
    type Place = ();

    fn place(&self) -> Option<()> {
        (self.held.get() < self.most).then(|| self.held.set(self.held.get() + 1))
    }

    fn fill(&self, (): (), job: Job) {
        self.jobs.borrow_mut().push_back(job);
        self.arrived.notify_one();
    }

    fn said(&self, destination: &DestinationName, said: String) {
        (self.hear)(format!("{}: {said}", destination.as_str()));
    }
}

struct World {
    config: Config,
    connected: Connected<Answers>,
    store: FakeStore,
    keyring: MemoryKeyring,
    answers: Answers,
    sockets: ScriptedSockets,
}

impl World {
    async fn new(kind: &str, answers: Answers, sockets: ScriptedSockets) -> Self {
        let config = config(kind);
        let prepared = prepare(&config, &secrets(), &FakeSigner, Uses::Serves)
            .await
            .unwrap();
        let connected = prepared.connect(|_| Ok::<_, ()>(answers.clone())).unwrap();
        let store = FakeStore::default();
        let shown = Shown {
            kind: Kind::RELEASE,
            attributes: herald_core::versioned("1.0.0", false),
            title: "gate".to_owned(),
            body: "Notes".to_owned(),
            links: BTreeMap::new(),
            occurred: "2027-01-15T08:00:00Z".parse().unwrap(),
        };
        let latest = Latest {
            project: "gate".to_owned(),
            newest: shown.clone(),
            stable: Some(shown),
            routes: ["discorded", "slacked", "roomed"]
                .into_iter()
                .map(RouteId::new)
                .collect::<BTreeSet<_>>(),
        };
        store.put_latest(&latest, None).await.unwrap();
        Self {
            config,
            connected,
            store,
            keyring: MemoryKeyring::new(8),
            answers,
            sockets,
        }
    }

    /// What the server said, run with at most `jobs` at once until it
    /// says something `done` holds true of.
    async fn served(&self, jobs: usize, done: impl Fn(&str) -> bool) -> Vec<String> {
        let clock = FakeClock::at("2027-01-15T08:00:00Z".parse().unwrap());
        let ports = Ports {
            store: &self.store,
            clock: &clock,
            signer: &FakeSigner,
        };
        let keys = keys_in(&self.keyring);
        let webhooks = Webhooks::default();
        let serving = Serving {
            config: &self.config,
            connected: &self.connected,
            ports: &ports,
            verifier: &FakeVerifier,
            public: &self.answers,
            keys: &keys,
            webhooks: &webhooks,
            admission: herald_flows::Admission::Once,
        };
        let heard = RefCell::new(Vec::new());
        let stop = Notify::new();
        let hear = |line: String| {
            if done(&line) {
                stop.notify_one();
            }
            heard.borrow_mut().push(line);
        };
        let taken = Taken {
            most: jobs,
            held: Cell::new(0),
            jobs: RefCell::default(),
            arrived: Notify::new(),
            hear: &hear,
        };
        let holding = Sessions::new(&serving, &taken, &Sleeping);
        let mut sessions = pin!(holding.held(&self.sockets));
        let mut held = true;
        let mut running: FuturesUnordered<Pin<Box<dyn Future<Output = Worked> + '_>>> =
            FuturesUnordered::new();
        let until = async {
            loop {
                let jobs: Vec<Job> = taken.jobs.borrow_mut().drain(..).collect();
                for job in jobs {
                    running.push(Box::pin(work(&serving, job, "test")));
                }
                tokio::select! {
                    () = stop.notified() => break,
                    () = &mut sessions, if held => held = false,
                    Some(worked) = running.next() => {
                        taken.held.set(taken.held.get() - 1);
                        hear(match worked.did {
                            Did::Interacted(interacted) => format!(
                                "answered {}: {:?}",
                                interacted[0].asked.command.name(),
                                interacted[0].sent
                            ),
                            other => format!("{other:?}"),
                        });
                    }
                    () = taken.arrived.notified() => {}
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(600), until)
            .await
            .expect("the sessions stopped within their bound");
        let heard = heard.borrow().clone();
        assert!(
            !heard
                .iter()
                .any(|line| line.starts_with("hooked:") || line.starts_with("pushed:")),
            "a destination whose commands are posted holds no session: {heard:?}"
        );
        heard
    }
}

fn text(value: &Value) -> Frame {
    Frame::Text(value.to_string())
}

fn interaction(kind: u64, data: &Value) -> Frame {
    text(&json!({ "op": 0, "s": 2, "t": "INTERACTION_CREATE", "d": {
        "id": "900",
        "token": "tok",
        "type": kind,
        "application_id": APPLICATION,
        "channel_id": CHANNEL,
        "user": { "id": "423456789012345678" },
        "data": data,
    }}))
}

fn in_guild(kind: u64, data: &Value) -> Frame {
    text(&json!({ "op": 0, "s": 2, "t": "INTERACTION_CREATE", "d": {
        "id": "900",
        "token": "tok",
        "type": kind,
        "application_id": APPLICATION,
        "channel_id": CHANNEL,
        "guild_id": GUILD,
        "member": { "user": { "id": "423456789012345678" }, "roles": [] },
        "data": data,
    }}))
}

fn hello(every: u64) -> Frame {
    text(&json!({ "op": 10, "d": { "heartbeat_interval": every } }))
}

fn ready() -> Frame {
    text(&json!({ "op": 0, "s": 1, "t": "READY", "d": {
        "session_id": "session",
        "resume_gateway_url": "wss://resume.discord.gg",
    }}))
}

fn answer(path: &str, status: u16, body: &str) -> Scripted {
    (path.to_owned(), Some((status, body.to_owned())))
}

fn sent(sockets: &ScriptedSockets) -> Vec<u64> {
    sockets
        .sent()
        .iter()
        .filter_map(|frame| {
            serde_json::from_str::<Value>(frame)
                .ok()?
                .get("op")?
                .as_u64()
        })
        .collect()
}

#[tokio::test(start_paused = true)]
async fn the_gateway_identifies_answers_a_command_and_its_completions_then_stops_where_told() {
    let sockets = ScriptedSockets::opening(vec![Ok(vec![
        hello(60_000),
        ready(),
        interaction(
            4,
            &json!({ "name": "latest", "options": [{ "name": "project", "type": 3, "value": "g", "focused": true }] }),
        ),
        interaction(2, &json!({ "name": "latest" })),
    ])]);
    let answers = Answers::of(vec![
        answer(&format!("/interactions/900/{TOKEN}/callback"), 204, ""),
        answer(&format!("/interactions/900/{TOKEN}/callback"), 204, ""),
        answer(
            &format!("/webhooks/{APPLICATION}/{TOKEN}/messages/@original"),
            200,
            "{}",
        ),
    ]);
    let world = World::new("discord on freebsd", answers, sockets.clone()).await;
    let heard = world.served(4, |line| line.starts_with("answered")).await;
    assert!(
        heard.contains(&"answered latest: Ok(())".to_owned()),
        "{heard:?}"
    );
    assert_eq!(sent(&sockets), [2]);
    let identified: Value = serde_json::from_str(&sockets.sent()[0]).unwrap();
    assert_eq!(identified["d"]["properties"]["os"], "freebsd");
    let seen = world.answers.seen();
    assert!(seen[0].ends_with("/callback"), "{seen:?}");
}

#[tokio::test(start_paused = true)]
async fn a_gateway_told_to_reconnect_resumes_and_stops_on_a_close_reconnecting_cannot_help() {
    let sockets = ScriptedSockets::opening(vec![
        Ok(vec![hello(60_000), ready(), text(&json!({ "op": 7 }))]),
        Ok(vec![hello(60_000), Frame::Closed(Some(4014))]),
    ]);
    let world = World::new("discord", Answers::default(), sockets.clone()).await;
    let heard = world.served(4, |line| line.contains("4014")).await;
    assert_eq!(
        sockets.opened(),
        [
            "wss://gateway.discord.gg/?v=10&encoding=json",
            "wss://resume.discord.gg/?v=10&encoding=json"
        ]
    );
    assert_eq!(sent(&sockets), [2, 6]);
    assert!(
        heard
            .iter()
            .any(|line| line
                .starts_with("discord: Discord's gateway closed the connection with 4014")),
        "{heard:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_gateway_that_cannot_be_reached_or_leaves_a_beat_unacknowledged_reconnects() {
    let sockets = ScriptedSockets::opening(vec![
        Err(HttpError::TimedOut),
        Ok(vec![hello(1_000)]),
        Ok(vec![Frame::Closed(Some(4004))]),
    ]);
    let world = World::new("discord", Answers::default(), sockets.clone()).await;
    let heard = world.served(4, |line| line.contains("4004")).await;
    assert!(
        heard.iter().any(|line| line
            == "discord: Discord's gateway could not be reached: the host did not answer in time"),
        "{heard:?}"
    );
    assert_eq!(sockets.opened().len(), 3);
    assert_eq!(sent(&sockets), [2, 1]);
}

#[tokio::test(start_paused = true)]
async fn a_gateway_that_refuses_what_is_sent_reconnects() {
    let sockets = ScriptedSockets::opening(vec![Ok(vec![hello(60_000)])]).refusing_after(0);
    let world = World::new("discord", Answers::default(), sockets.clone()).await;
    let heard = world
        .served(4, |line| line.contains("could not be reached"))
        .await;
    assert_eq!(sockets.opened().len(), 2, "{heard:?}");
}

#[tokio::test(start_paused = true)]
async fn a_beat_the_gateway_refuses_reconnects() {
    let sockets = ScriptedSockets::opening(vec![Ok(vec![hello(1_000)])]).refusing_after(1);
    let world = World::new("discord", Answers::default(), sockets.clone()).await;
    let heard = world
        .served(4, |line| line.contains("could not be reached"))
        .await;
    assert_eq!(sockets.sent().len(), 1, "{heard:?}");
    assert_eq!(sockets.opened().len(), 2, "{heard:?}");
}

#[tokio::test(start_paused = true)]
async fn completions_that_cannot_be_sent_are_said_and_a_ping_passes() {
    let sockets = ScriptedSockets::opening(vec![Ok(vec![
        hello(60_000),
        interaction(1, &json!({})),
        interaction(4, &json!({ "name": "latest", "options": [] })),
    ])]);
    let world = World::new("discord", Answers::default(), sockets).await;
    let heard = world
        .served(4, |line| line.contains("completions could not be sent"))
        .await;
    assert!(
        heard.iter().any(|line| line
            .starts_with("discord: completions could not be sent: the host could not be reached")),
        "{heard:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn completions_past_the_bound_are_dropped_and_said() {
    let completing = interaction(4, &json!({ "name": "latest", "options": [] }));
    let mut frames = vec![hello(60_000)];
    frames.extend(std::iter::repeat_n(completing, 5));
    let sockets = ScriptedSockets::opening(vec![Ok(frames)]);
    let hanging = (0..4)
        .map(|_| (format!("/interactions/900/{TOKEN}/callback"), None))
        .collect();
    let world = World::new("discord", Answers::of(hanging), sockets).await;
    let heard = world
        .served(4, |line| line.contains("a completion was dropped"))
        .await;
    assert_eq!(
        heard,
        ["discord: a completion was dropped: no more are answered at once"]
    );
}

#[tokio::test(start_paused = true)]
async fn a_gateway_that_stops_finishes_the_answers_it_was_giving() {
    let sockets = ScriptedSockets::opening(vec![Ok(vec![
        hello(60_000),
        interaction(2, &json!({ "name": "latest" })),
        Frame::Closed(Some(4004)),
    ])]);
    let answers = Answers::of(vec![
        answer(&format!("/interactions/900/{TOKEN}/callback"), 204, ""),
        answer(
            &format!("/webhooks/{APPLICATION}/{TOKEN}/messages/@original"),
            200,
            "{}",
        ),
    ]);
    let world = World::new("discord", answers, sockets).await;
    let heard = world.served(4, |line| line.starts_with("answered")).await;
    let stopped = heard.iter().position(|line| line.contains("4004"));
    let answered = heard.iter().position(|line| line.starts_with("answered"));
    assert!(stopped < answered, "{heard:?}");
}

#[tokio::test(start_paused = true)]
async fn the_gateway_defers_a_command_before_its_work_is_done() {
    let sockets = ScriptedSockets::opening(vec![Ok(vec![
        hello(60_000),
        ready(),
        in_guild(2, &json!({ "name": "subscribe" })),
    ])]);
    let answers = Answers::of(vec![
        answer(&format!("/interactions/900/{TOKEN}/callback"), 204, ""),
        answer(&format!("/guilds/{GUILD}/members/"), 204, ""),
        answer(
            &format!("/webhooks/{APPLICATION}/{TOKEN}/messages/@original"),
            200,
            "{}",
        ),
    ]);
    let world = World::new("discord", answers, sockets).await;
    let heard = world.served(4, |line| line.starts_with("answered")).await;
    assert!(
        heard.contains(&"answered subscribe: Ok(())".to_owned()),
        "{heard:?}"
    );
    let seen = world.answers.seen();
    assert!(seen[0].ends_with("/callback"), "{seen:?}");
    assert!(
        seen[1].starts_with("PUT ") && seen[1].contains("/roles/"),
        "{seen:?}"
    );
    assert!(seen[2].contains("@original"), "{seen:?}");
}

#[tokio::test(start_paused = true)]
async fn a_command_the_gateway_cannot_defer_is_said_and_not_worked() {
    let sockets = ScriptedSockets::opening(vec![Ok(vec![
        hello(60_000),
        interaction(2, &json!({ "name": "latest" })),
    ])]);
    let world = World::new("discord", Answers::default(), sockets).await;
    let heard = world
        .served(4, |line| line.contains("could not be deferred"))
        .await;
    assert!(
        heard
            .iter()
            .any(|line| line
                .starts_with("discord: latest was not answered: it could not be deferred: ")),
        "{heard:?}"
    );
    assert_eq!(world.answers.seen().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_command_taken_up_just_before_the_gateway_is_lost_is_deferred_while_it_reconnects() {
    let sockets = ScriptedSockets::opening(vec![
        Ok(vec![
            hello(60_000),
            interaction(2, &json!({ "name": "latest" })),
            Frame::Closed(None),
        ]),
        Err(HttpError::TimedOut),
    ]);
    let answers = Answers::of(vec![
        answer(&format!("/interactions/900/{TOKEN}/callback"), 204, ""),
        answer(
            &format!("/webhooks/{APPLICATION}/{TOKEN}/messages/@original"),
            200,
            "{}",
        ),
    ]);
    let world = World::new("discord", answers, sockets).await;
    let answered = std::cell::Cell::new(false);
    let unreached = std::cell::Cell::new(false);
    let heard = world
        .served(4, |line| {
            answered.set(answered.get() || line.starts_with("answered"));
            unreached.set(unreached.get() || line.contains("could not be reached"));
            answered.get() && unreached.get()
        })
        .await;
    let answered = heard.iter().position(|line| line.starts_with("answered"));
    let unreached = heard
        .iter()
        .position(|line| line.contains("could not be reached"));
    assert!(answered < unreached, "{heard:?}");
}

fn envelope(text: &str) -> Frame {
    Frame::Text(
        json!({ "envelope_id": "e1", "type": "slash_commands", "payload": {
            "command": "/herald",
            "text": text,
            "channel_id": "C0123456789",
            "user_id": "U0123456789",
            "response_url": "https://hooks.slack.com/commands/T1/2/abc",
        }})
        .to_string(),
    )
}

#[tokio::test(start_paused = true)]
async fn socket_mode_acknowledges_answers_and_reopens_when_slack_disconnects() {
    let sockets = ScriptedSockets::opening(vec![Ok(vec![
        envelope("latest"),
        Frame::Text(r#"{"type": "disconnect"}"#.to_owned()),
    ])]);
    let answers = Answers::of(vec![
        answer(
            "/apps.connections.open",
            200,
            r#"{"ok": true, "url": "wss://wss-primary.slack.com/link"}"#,
        ),
        answer("«secret:the command's response URL»", 200, "ok"),
        answer(
            "/apps.connections.open",
            200,
            r#"{"ok": false, "error": "invalid_auth"}"#,
        ),
    ]);
    let world = World::new("slack", answers, sockets.clone()).await;
    let heard = world.served(4, |line| line.contains("invalid_auth")).await;
    assert_eq!(sockets.opened(), ["wss://wss-primary.slack.com/link"]);
    assert_eq!(sockets.sent(), [r#"{"envelope_id":"e1"}"#]);
    assert!(heard.contains(&"slack: Slack's Socket Mode could not be opened: Slack opened no Socket Mode connection: invalid_auth".to_owned()), "{heard:?}");
}

#[tokio::test(start_paused = true)]
async fn socket_mode_that_refuses_an_acknowledgement_or_cannot_be_opened_reopens() {
    let sockets =
        ScriptedSockets::opening(vec![Ok(vec![envelope("latest")]), Err(HttpError::TimedOut)])
            .refusing_after(0);
    let opening = || {
        answer(
            "/apps.connections.open",
            200,
            r#"{"ok": true, "url": "wss://wss-primary.slack.com/link"}"#,
        )
    };
    let answers = Answers::of(vec![opening(), opening()]);
    let world = World::new("slack", answers, sockets.clone()).await;
    let heard = world
        .served(4, |line| line.contains("nothing scripted"))
        .await;
    assert_eq!(sockets.opened().len(), 2, "{heard:?}");
    assert!(sockets.sent().is_empty(), "{heard:?}");
    let opened: Vec<&str> = heard
        .iter()
        .filter_map(|line| line.strip_prefix("slack: Slack's Socket Mode could not be opened: "))
        .collect();
    assert_eq!(
        opened,
        [
            "the host did not answer in time",
            "the host could not be reached: nothing scripted for POST https://slack.com/api/apps.connections.open",
        ],
        "{heard:?}"
    );
    assert!(
        !heard.iter().any(|line| line.starts_with("answered")),
        "{heard:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_sync_the_homeserver_answers_unreadably_or_not_at_all_is_tried_again() {
    let answers = Answers::of(vec![answer("sync?timeout=0", 200, "not json")]);
    let world = World::new("matrix", answers, ScriptedSockets::default()).await;
    let failures = std::cell::Cell::new(0);
    let heard = world
        .served(4, |line| {
            if line.contains("could not be synced") {
                failures.set(failures.get() + 1);
            }
            failures.get() == 2
        })
        .await;
    let failed: Vec<&String> = heard
        .iter()
        .filter(|line| line.starts_with("matrix: the homeserver could not be synced: "))
        .collect();
    assert_eq!(failed.len(), 2, "{heard:?}");
    assert!(failed[1].contains("nothing scripted"), "{heard:?}");
}

#[tokio::test(start_paused = true)]
async fn socket_mode_waits_longer_after_each_connection_slack_does_not_greet() {
    let closed = || Ok(vec![Frame::Closed(None)]);
    let greeted = Ok(vec![
        Frame::Text(r#"{"type": "hello"}"#.to_owned()),
        Frame::Closed(None),
    ]);
    let sockets = ScriptedSockets::opening(vec![closed(), closed(), greeted, closed()]);
    let opening = || {
        answer(
            "/apps.connections.open",
            200,
            r#"{"ok": true, "url": "wss://wss-primary.slack.com/link"}"#,
        )
    };
    let answers = Answers::of(vec![
        opening(),
        opening(),
        opening(),
        opening(),
        answer(
            "/apps.connections.open",
            200,
            r#"{"ok": false, "error": "invalid_auth"}"#,
        ),
    ]);
    let world = World::new("slack", answers, sockets).await;
    world.served(4, |line| line.contains("invalid_auth")).await;
    assert_eq!(world.answers.spaced("/apps.connections.open"), [1, 2, 1, 2]);
}

#[tokio::test(start_paused = true)]
async fn a_command_beyond_what_the_server_answers_at_once_is_said_and_left() {
    let sockets = ScriptedSockets::opening(vec![Ok(vec![
        envelope("latest"),
        envelope("changelog"),
        envelope("subscribe"),
    ])]);
    let answers = Answers::of(vec![
        answer(
            "/apps.connections.open",
            200,
            r#"{"ok": true, "url": "wss://wss-primary.slack.com/link"}"#,
        ),
        answer("«secret:the command's response URL»", 200, "ok"),
    ]);
    let world = World::new("slack", answers, sockets).await;
    let heard = world
        .served(1, |line| line.contains("was not answered"))
        .await;
    let refused: Vec<&String> = heard
        .iter()
        .filter(|line| line.ends_with("was not answered: no more is answered at once"))
        .collect();
    assert!(!refused.is_empty(), "{heard:?}");
    assert!(
        refused
            .iter()
            .all(|line| !line.starts_with("slack: latest")),
        "the first command is taken: {heard:?}"
    );
}

fn synced(next: &str, body: &str) -> String {
    json!({ "next_batch": next, "rooms": { "join": { "!room:matrix.example": { "timeline": { "events": [{
            "type": "m.room.message",
            "room_id": "!room:matrix.example",
            "event_id": "$command",
            "sender": "@ann:matrix.example",
            "content": { "msgtype": "m.text", "body": body },
        }]}}}}})
        .to_string()
}

#[tokio::test(start_paused = true)]
async fn a_sync_skips_history_answers_what_follows_and_waits_after_a_failure() {
    let answers = Answers::of(vec![
        answer("sync?timeout=0", 200, &synced("s1", "!herald latest")),
        answer("since=s1", 200, &synced("s2", "!herald latest")),
        answer("/send/m.room.message/", 200, "{}"),
        answer("since=s2", 500, r#"{"errcode": "M_UNKNOWN"}"#),
    ]);
    let world = World::new("matrix", answers, ScriptedSockets::default()).await;
    let answered = std::cell::Cell::new(false);
    let failed = std::cell::Cell::new(false);
    let heard = world
        .served(4, |line| {
            answered.set(answered.get() || line.starts_with("answered"));
            failed.set(failed.get() || line.contains("could not be synced"));
            answered.get() && failed.get()
        })
        .await;
    assert!(
        heard.contains(&"answered latest: Ok(())".to_owned()),
        "{heard:?}"
    );
    assert!(
        heard.contains(
            &"matrix: the homeserver could not be synced: Matrix refused it with 500: M_UNKNOWN"
                .to_owned()
        ),
        "{heard:?}"
    );
    assert_eq!(
        heard
            .iter()
            .filter(|line| line.starts_with("answered"))
            .count(),
        1
    );
}

#[tokio::test(start_paused = true)]
async fn a_sync_waits_from_the_first_again_once_the_homeserver_answers() {
    let failed = || answer("/sync?", 500, r#"{"errcode": "M_UNKNOWN"}"#);
    let answers = Answers::of(vec![
        failed(),
        failed(),
        answer("sync?timeout=0", 200, &synced("s1", "hi")),
        failed(),
    ]);
    let world = World::new("matrix", answers, ScriptedSockets::default()).await;
    let failures = std::cell::Cell::new(0);
    world
        .served(4, |line| {
            if line.contains("could not be synced") {
                failures.set(failures.get() + 1);
            }
            failures.get() == 4
        })
        .await;
    assert_eq!(world.answers.spaced("/sync?"), [1, 2, 0, 1]);
}
