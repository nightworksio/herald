//! `herald server` over real connections: health, the limits it holds a
//! request to, the work a webhook leaves, its rounds, and stopping.

#[cfg(test)]
#[allow(
    clippy::unused_async_trait_impl,
    reason = "a fake answers at once, and is written as the async trait it fakes"
)]
mod server {
    use std::cell::{Cell, RefCell};
    use std::fmt::Write as _;
    use std::io;
    use std::net::SocketAddr;
    use std::pin::Pin;
    use std::time::Duration;

    use futures_util::FutureExt as _;
    use herald_compose::Capped;
    use herald_config::{Config, load};
    use herald_core::{DestinationName, Scheme, Subscription};
    use herald_flows::{Connected, Keys, Ports, Serving, Webhooks, prepare, webhooks};
    use herald_ports::{
        Admitted, Clock as _, Http, HttpError, Keyring as _, KnownKey, Request, Response,
        Store as _,
    };
    use herald_records::MemoryKeyring;
    use herald_server::{Asked, Heard, Limits, Listener, serve};
    use herald_testing::{
        FakeClock, FakeSecrets, FakeSigner, FakeStore, FakeVerifier, ScriptedSockets,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::Notify;

    const EVENT: &str =
        include_str!("../../herald-sources-forge/fixtures/github/release-event.reference.json");

    /// How long a test lets the server run before failing it.
    const BOUND: Duration = Duration::from_secs(5);

    fn config() -> Config {
        load(
            "herald.toml",
            r#"version = 1
[store]
kind = "sqlite"
[destinations.fedi]
kind = "activitypub"
domain = "herald.example"
key = { secret = "HERALD_ACTIVITYPUB_KEY" }
[sources.gate]
kind = "github"
repository = "cli/cli"
webhook = { secret = "HERALD_WEBHOOK" }
[[routes]]
id = "followed"
sources = ["gate"]
destination = "fedi"
template = "t"
actor = "php-mutation-gate"
[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
"#,
            &|_| Err(String::new()),
        )
        .unwrap()
    }

    fn secrets() -> FakeSecrets {
        FakeSecrets::default()
            .with("HERALD_ACTIVITYPUB_KEY", "the-key")
            .with("HERALD_WEBHOOK", "hook-secret")
    }

    /// An `Http` that answers every request 202 once `open` lets it.
    #[derive(Debug, Clone, Default)]
    struct Gate {
        open: std::sync::Arc<Notify>,
        gated: bool,
    }

    impl Http for Gate {
        async fn send(&self, _: Request) -> Result<Response, HttpError> {
            if self.gated {
                self.open.notified().await;
            }
            Ok(Response {
                status: 202,
                headers: Vec::new(),
                body: Vec::new(),
            })
        }
    }

    struct World {
        config: Config,
        connected: Connected<Gate>,
        webhooks: Webhooks,
        store: FakeStore,
        keyring: MemoryKeyring,
        public: Capped<Gate>,
        clock: FakeClock,
        sockets: ScriptedSockets,
    }

    impl World {
        async fn new(http: Gate) -> Self {
            let config = config();
            let prepared = prepare(&config, &secrets(), &FakeSigner).await.unwrap();
            let connected = prepared.connect(|_| Ok::<_, ()>(http.clone())).unwrap();
            let webhooks = webhooks(&config, &secrets()).await.unwrap();
            Self {
                config,
                connected,
                webhooks,
                store: FakeStore::default(),
                keyring: MemoryKeyring::new(8),
                public: Capped::new(http, 2),
                clock: FakeClock::at("2027-01-15T08:00:00Z".parse().unwrap()),
                sockets: ScriptedSockets::default(),
            }
        }

        async fn serve(
            &self,
            listener: Taking,
            limits: Limits,
            heard: &dyn Fn(Heard),
            stop: impl Future<Output = ()>,
        ) {
            let ports = Ports {
                store: &self.store,
                clock: &self.clock,
                signer: &FakeSigner,
            };
            let keys = Keys {
                keyring: &self.keyring,
                refreshed: jiff::SignedDuration::from_hours(1),
            };
            let serving = Serving {
                config: &self.config,
                connected: &self.connected,
                ports: &ports,
                verifier: &FakeVerifier,
                public: &self.public,
                keys: &keys,
                webhooks: &self.webhooks,
                admission: herald_flows::Admission::Once,
            };
            let stop: Pin<Box<dyn Future<Output = ()> + '_>> = Box::pin(stop);
            tokio::time::timeout(
                BOUND,
                serve(&serving, listener, &self.sockets, limits, heard, stop),
            )
            .await
            .expect("the server stopped within its bound");
        }
    }

    fn limits() -> Limits {
        Limits {
            jobs: 4,
            connections: 4,
            body: 1 << 20,
            poll: None,
            housekeep: None,
            keys: Duration::from_secs(3_600),
            read: Duration::from_secs(2),
            asked: None,
        }
    }

    /// A listener on a free local port, and where it listens.
    async fn listening() -> (Taking, SocketAddr) {
        let inner = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = inner.local_addr().unwrap();
        let listener = Taking {
            inner,
            failing: Cell::new(false),
            addressed: true,
        };
        (listener, address)
    }

    /// What the server at `address` answers `request`, whole.
    async fn asked(address: SocketAddr, request: &[u8]) -> String {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(request).await.unwrap();
        let mut answer = Vec::new();
        stream.read_to_end(&mut answer).await.unwrap();
        String::from_utf8(answer).unwrap()
    }

    fn posted(path: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
        let lines = headers
            .iter()
            .fold(String::new(), |mut lines, (name, value)| {
                let _ = write!(lines, "{name}: {value}\r\n");
                lines
            });
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: herald.example\r\nContent-Length: {}\r\n{lines}\r\n",
            body.len()
        );
        [request.as_bytes(), body].concat()
    }

    fn signature(body: &[u8]) -> String {
        format!(
            "sha256={}",
            herald_core::hex(&[b"HmacSha256|hook-secret|".as_slice(), body].concat())
        )
    }

    fn release(body: &[u8]) -> Vec<u8> {
        let signed = signature(body);
        posted(
            "/hooks/gate",
            &[
                ("X-Hub-Signature-256", signed.as_str()),
                ("X-GitHub-Event", "release"),
            ],
            body,
        )
    }

    fn said(heard: &RefCell<Vec<String>>) -> Vec<String> {
        heard.borrow().clone()
    }

    fn named(heard: &Heard) -> String {
        match heard {
            Heard::Listening(_) => "listening".to_owned(),
            Heard::Worked(_) => "worked".to_owned(),
            Heard::Polled(_) => "polled".to_owned(),
            Heard::Kept(_) => "kept".to_owned(),
            Heard::Unaccepted(reason) => format!("unaccepted: {reason}"),
            Heard::Refused(reason) => format!("refused: {reason}"),
            Heard::Session { destination, said } => format!("{}: {said}", destination.as_str()),
        }
    }

    #[tokio::test]
    async fn health_is_answered_over_http_and_the_server_says_where_it_listens() {
        let world = World::new(Gate::default()).await;
        let (listener, address) = listening().await;
        let heard = RefCell::new(Vec::new());
        let stop = Notify::new();
        let client = async {
            let answer = asked(address, b"GET /healthz HTTP/1.1\r\nHost: x\r\n\r\n").await;
            stop.notify_one();
            answer
        };
        let hear = |said: Heard| heard.borrow_mut().push(named(&said));
        let (answer, ()) = tokio::join!(
            client,
            world.serve(listener, limits(), &hear, stop.notified())
        );
        assert!(answer.starts_with("HTTP/1.1 200 OK\r\n"), "{answer}");
        assert!(
            answer.contains("content-type: text/plain; charset=utf-8"),
            "{answer}"
        );
        assert!(answer.ends_with("herald is up"), "{answer}");
        assert_eq!(said(&heard), ["listening"]);
    }

    #[tokio::test]
    async fn a_body_larger_than_the_limit_is_refused() {
        let world = World::new(Gate::default()).await;
        let (listener, address) = listening().await;
        let stop = Notify::new();
        let client = async {
            let answer = asked(address, &posted("/inbox", &[], b"0123456789")).await;
            stop.notify_one();
            answer
        };
        let small = Limits {
            body: 4,
            ..limits()
        };
        let (answer, ()) = tokio::join!(
            client,
            world.serve(listener, small, &|_| {}, stop.notified())
        );
        assert!(answer.starts_with("HTTP/1.1 413 "), "{answer}");
    }

    #[tokio::test]
    async fn a_body_that_is_not_sent_in_time_is_refused() {
        let world = World::new(Gate::default()).await;
        let (listener, address) = listening().await;
        let stop = Notify::new();
        let client = async {
            let answer = asked(
                address,
                b"POST /inbox HTTP/1.1\r\nHost: x\r\nContent-Length: 10\r\n\r\n01",
            )
            .await;
            stop.notify_one();
            answer
        };
        let quick = Limits {
            read: Duration::from_millis(100),
            asked: None,
            ..limits()
        };
        let (answer, ()) = tokio::join!(
            client,
            world.serve(listener, quick, &|_| {}, stop.notified())
        );
        assert!(answer.starts_with("HTTP/1.1 408 "), "{answer}");
    }

    #[tokio::test]
    async fn work_the_queue_has_no_place_for_is_refused_and_its_delivery_released() {
        let world = World::new(Gate::default()).await;
        let heard = RefCell::new(Vec::new());
        let hear = |said: Heard| heard.borrow_mut().push(named(&said));
        let unsigned = Limits {
            jobs: 0,
            ..limits()
        };
        let (listener, address) = listening().await;
        let stop = Notify::new();
        let client = async {
            let answers = (
                asked(address, &posted("/inbox", &[], b"{}")).await,
                asked(address, &release(EVENT.as_bytes())).await,
            );
            stop.notify_one();
            answers
        };
        let ((unread, busy), ()) = tokio::join!(
            client,
            world.serve(listener, unsigned, &hear, stop.notified())
        );
        assert!(unread.starts_with("HTTP/1.1 401 "), "{unread}");
        assert!(busy.starts_with("HTTP/1.1 503 "), "{busy}");
        assert!(busy.contains("retry-after: 30"), "{busy}");
        assert!(
            busy.ends_with("herald cannot answer this request now"),
            "{busy}"
        );
        assert_eq!(
            said(&heard),
            [
                "listening",
                "refused: the request carries no Signature",
                "refused: the queue has no place for the work it leaves",
            ]
        );
        let (listener, address) = listening().await;
        let stop = Notify::new();
        let client = async {
            let answer = asked(address, &release(EVENT.as_bytes())).await;
            stop.notify_one();
            answer
        };
        let (answer, ()) = tokio::join!(
            client,
            world.serve(listener, limits(), &|_| {}, stop.notified())
        );
        assert!(answer.starts_with("HTTP/1.1 202 "), "{answer}");
    }

    #[tokio::test]
    async fn a_webhook_s_release_is_announced_once_it_is_answered() {
        let world = World::new(Gate::default()).await;
        let (listener, address) = listening().await;
        let heard = RefCell::new(Vec::new());
        let stop = Notify::new();
        let client = async { asked(address, &release(EVENT.as_bytes())).await };
        let hear = |said: Heard| {
            let name = named(&said);
            if name == "worked" {
                stop.notify_one();
            }
            heard.borrow_mut().push(name);
        };
        let (answer, ()) = tokio::join!(
            client,
            world.serve(listener, limits(), &hear, stop.notified())
        );
        assert!(answer.starts_with("HTTP/1.1 202 "), "{answer}");
        assert_eq!(said(&heard), ["listening", "worked"]);
    }

    #[tokio::test]
    async fn a_refused_request_is_answered_without_its_reason_and_the_reason_is_said() {
        let world = World::new(Gate::default()).await;
        let (listener, address) = listening().await;
        let heard = RefCell::new(Vec::new());
        let stop = Notify::new();
        let forged = posted(
            "/hooks/gate",
            &[
                ("X-Hub-Signature-256", "sha256=00"),
                ("X-GitHub-Event", "release"),
            ],
            EVENT.as_bytes(),
        );
        let client = async {
            let answer = asked(address, &forged).await;
            stop.notify_one();
            answer
        };
        let hear = |said: Heard| heard.borrow_mut().push(named(&said));
        let (answer, ()) = tokio::join!(
            client,
            world.serve(listener, limits(), &hear, stop.notified())
        );
        assert!(answer.starts_with("HTTP/1.1 401 "), "{answer}");
        assert!(
            answer.ends_with("herald does not take this request"),
            "{answer}"
        );
        let said = said(&heard);
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(said[1].starts_with("refused: "), "{said:?}");
    }

    #[tokio::test]
    async fn stopping_releases_the_work_waiting_and_finishes_the_work_running() {
        let gate = Gate {
            gated: true,
            ..Gate::default()
        };
        let world = World::new(gate.clone()).await;
        world
            .store
            .subscribe(&Subscription {
                destination: DestinationName::new("fedi"),
                project: "php-mutation-gate".to_owned(),
                subscriber: "https://social.example/users/ana".to_owned(),
                inbox: Some("https://social.example/inbox".to_owned()),
            })
            .await
            .unwrap();
        let (listener, address) = listening().await;
        let heard = RefCell::new(Vec::new());
        let stop = Notify::new();
        let later = EVENT.replace("\"tag_name\": \"v2.103.3\"", "\"tag_name\": \"v2.103.4\"");
        assert_ne!(later, EVENT);
        let client = async {
            let first = asked(address, &release(EVENT.as_bytes())).await;
            let second = asked(address, &release(later.as_bytes())).await;
            stop.notify_one();
            (first, second)
        };
        let one = Limits {
            jobs: 1,
            ..limits()
        };
        let hear = |said: Heard| heard.borrow_mut().push(named(&said));
        let stopped = async {
            stop.notified().await;
            gate.open.notify_one();
        };
        let ((first, second), ()) =
            tokio::join!(client, world.serve(listener, one, &hear, stopped));
        assert!(first.starts_with("HTTP/1.1 202 "), "{first}");
        assert!(second.starts_with("HTTP/1.1 202 "), "{second}");
        assert_eq!(said(&heard), ["listening", "worked"]);
        let at = world.clock.now();
        let first = signature(EVENT.as_bytes());
        let second = signature(later.as_bytes());
        let delivery = |signed: &str| signed.trim_start_matches("sha256=").to_owned();
        assert_eq!(
            world
                .store
                .admit(Scheme::GitHub, &delivery(&first), at)
                .await,
            Ok(Admitted::Seen)
        );
        assert_eq!(
            world
                .store
                .admit(Scheme::GitHub, &delivery(&second), at)
                .await,
            Ok(Admitted::Fresh)
        );
    }

    #[tokio::test]
    async fn the_rounds_poll_keep_house_and_forget_old_keys_one_of_each_at_a_time() {
        let world = World::new(Gate::default()).await;
        let (listener, _) = listening().await;
        let heard = RefCell::new(Vec::new());
        let stop = Notify::new();
        let rounds = Limits {
            poll: Some(Duration::from_millis(5)),
            housekeep: Some(Duration::from_millis(5)),
            keys: Duration::from_millis(5),
            ..limits()
        };
        let old = "https://social.example/users/ana#main-key";
        world
            .keyring
            .know(
                old,
                &KnownKey {
                    owner: "https://social.example/users/ana".to_owned(),
                    pem: "pem".to_owned(),
                    fetched: "2027-01-14T08:00:00Z".parse().unwrap(),
                },
            )
            .await
            .unwrap();
        let forgotten = || matches!(world.keyring.known(old).now_or_never(), Some(Ok(None)));
        let seen = Cell::new((false, false));
        world
            .serve(
                listener,
                rounds,
                &|said| {
                    let name = named(&said);
                    let (polled, kept) = seen.get();
                    seen.set((polled || name == "polled", kept || name == "kept"));
                    if seen.get() == (true, true) && forgotten() {
                        stop.notify_one();
                    }
                    heard.borrow_mut().push(name);
                },
                stop.notified(),
            )
            .await;
        assert!(said(&heard).contains(&"polled".to_owned()));
        assert!(said(&heard).contains(&"kept".to_owned()));
        assert!(forgotten());
    }

    #[tokio::test]
    async fn a_host_asks_for_rounds_at_its_paths_and_each_runs_one_at_a_time() {
        let gate = Gate {
            open: std::sync::Arc::new(Notify::new()),
            gated: true,
        };
        let world = World::new(gate.clone()).await;
        let (listener, address) = listening().await;
        let heard = RefCell::new(Vec::new());
        let stop = Notify::new();
        let asking = Limits {
            asked: Some(Asked {
                poll: "/poll",
                housekeep: "/housekeep",
                reply: r#"{"Outputs":{}}"#,
            }),
            ..limits()
        };
        let client = async {
            let poll = posted("/poll", &[], b"{}");
            let first = asked(address, &poll);
            let rest = async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                let again = asked(address, &posted("/poll", &[], b"{}")).await;
                let kept = asked(address, &posted("/housekeep", &[], b"{}")).await;
                let elsewhere = asked(address, &posted("/nowhere", &[], b"{}")).await;
                (again, kept, elsewhere)
            };
            let opening = async {
                loop {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    gate.open.notify_waiters();
                }
            };
            let answers = tokio::select! {
                answers = async { tokio::join!(first, rest) } => answers,
                () = opening => unreachable!(),
            };
            stop.notify_one();
            answers
        };
        let hear = |said: Heard| heard.borrow_mut().push(named(&said));
        let ((first, (again, kept, elsewhere)), ()) = tokio::join!(
            client,
            world.serve(listener, asking, &hear, stop.notified())
        );
        for answer in [&first, &again, &kept] {
            assert!(answer.starts_with("HTTP/1.1 200 OK\r\n"), "{answer}");
            assert!(answer.ends_with(r#"{"Outputs":{}}"#), "{answer}");
            assert!(
                answer.contains("content-type: application/json"),
                "{answer}"
            );
        }
        assert!(elsewhere.starts_with("HTTP/1.1 404"), "{elsewhere}");
        let said = said(&heard);
        assert_eq!(
            said.iter().filter(|name| *name == "polled").count(),
            1,
            "{said:?}"
        );
        assert!(said.contains(&"kept".to_owned()), "{said:?}");
    }

    /// A TCP listener whose next connection may fail to be taken, and
    /// whose address may not be known: every test serves through it, so
    /// one instance of `serve` takes every path.
    struct Taking {
        inner: TcpListener,
        failing: Cell<bool>,
        addressed: bool,
    }

    impl Listener for Taking {
        type Stream = TcpStream;

        async fn take(&self) -> io::Result<TcpStream> {
            if self.failing.replace(false) {
                return Err(io::Error::other("too many open files"));
            }
            self.inner.take().await
        }

        fn address(&self) -> io::Result<SocketAddr> {
            if self.addressed {
                self.inner.address()
            } else {
                Err(io::Error::other("no address"))
            }
        }
    }

    #[tokio::test]
    async fn a_connection_that_cannot_be_taken_is_said_and_the_next_is_taken() {
        let world = World::new(Gate::default()).await;
        let (listening, address) = listening().await;
        let flaky = Taking {
            failing: Cell::new(true),
            addressed: false,
            ..listening
        };
        let heard = RefCell::new(Vec::new());
        let stop = Notify::new();
        let client = async {
            let answer = asked(address, b"GET /healthz HTTP/1.1\r\nHost: x\r\n\r\n").await;
            stop.notify_one();
            answer
        };
        let hear = |said: Heard| heard.borrow_mut().push(named(&said));
        let (answer, ()) =
            tokio::join!(client, world.serve(flaky, limits(), &hear, stop.notified()));
        assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
        assert_eq!(said(&heard), ["unaccepted: too many open files"]);
    }
}
