//! The sessions `herald server` holds: Discord's gateway, Slack's Socket
//! Mode and a Matrix sync, each answering its commands and reconnecting.

#[cfg(test)]
mod sessions {
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet, VecDeque};
    use std::future::Future;
    use std::io;
    use std::net::SocketAddr;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use herald_config::{Config, load};
    use herald_core::{Kind, Latest, RouteId, Shown};
    use herald_flows::{Connected, Keys, Ports, Serving, Webhooks, Worked, prepare};
    use herald_ports::{Frame, Http, HttpError, Request, Response, Store as _};
    use herald_records::MemoryKeyring;
    use herald_server::{Heard, Limits, Listener, serve};
    use herald_testing::{
        FakeClock, FakeSecrets, FakeSigner, FakeStore, FakeVerifier, ScriptedSockets,
    };
    use serde_json::json;
    use tokio::net::TcpStream;
    use tokio::sync::Notify;

    const APPLICATION: &str = "123456789012345678";
    const CHANNEL: &str = "323456789012345678";
    const ROLE: &str = "523456789012345678";
    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// A config whose one destination, `kind`, holds a session.
    fn config(kind: &str) -> Config {
        let destination = match kind {
            "discord" => format!(
                r#"[destinations.discord]
kind = "discord"
token = {{ secret = "HERALD_DISCORD_TOKEN" }}
[destinations.discord.interactions]
application = "{APPLICATION}"
via = "gateway"
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
title = "{{{{ event.project }}}} {{{{ event.version }}}}"
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
        load("herald.toml", &text, &|_| Err(String::new())).unwrap()
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

    /// A listener no one connects to.
    struct Nobody;

    impl Listener for Nobody {
        type Stream = TcpStream;

        async fn take(&self) -> io::Result<TcpStream> {
            std::future::pending().await
        }

        fn address(&self) -> io::Result<SocketAddr> {
            Err(io::Error::other("nowhere"))
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
            let prepared = prepare(&config, &secrets(), &FakeSigner)
                .await
                .unwrap()
                .answering(&secrets())
                .await
                .unwrap();
            let connected = prepared.connect(|_| Ok::<_, ()>(answers.clone())).unwrap();
            let store = FakeStore::default();
            let shown = Shown {
                kind: Kind::Release,
                version: Some("1.0.0".to_owned()),
                title: "gate".to_owned(),
                body: "Notes".to_owned(),
                links: BTreeMap::new(),
                prerelease: false,
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
            let keys = Keys {
                keyring: &self.keyring,
                refreshed: jiff::SignedDuration::from_hours(1),
            };
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
            let limits = Limits {
                jobs,
                connections: 1,
                body: 1 << 20,
                poll: None,
                housekeep: None,
                keys: Duration::from_secs(3_600),
                read: Duration::from_secs(2),
                asked: None,
            };
            let heard = RefCell::new(Vec::new());
            let stop = Notify::new();
            let hear = |said: Heard| {
                let line = match said {
                    Heard::Session { destination, said } => {
                        format!("{}: {said}", destination.as_str())
                    }
                    Heard::Worked(Worked::Interacted(interacted)) => format!(
                        "answered {}: {:?}",
                        interacted[0].asked.command.name(),
                        interacted[0].sent
                    ),
                    other => format!("{other:?}"),
                };
                if done(&line) {
                    stop.notify_one();
                }
                heard.borrow_mut().push(line);
            };
            let stopped: Pin<Box<dyn Future<Output = ()> + '_>> = Box::pin(stop.notified());
            tokio::time::timeout(
                Duration::from_secs(600),
                serve(&serving, Nobody, &self.sockets, limits, &hear, stopped),
            )
            .await
            .expect("the server stopped within its bound");
            let heard = heard.into_inner();
            assert!(
                !heard
                    .iter()
                    .any(|line| line.starts_with("hooked:") || line.starts_with("pushed:")),
                "a destination whose commands are posted holds no session: {heard:?}"
            );
            heard
        }
    }

    fn answer(path: &str, status: u16, body: &str) -> Scripted {
        (path.to_owned(), Some((status, body.to_owned())))
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
}
