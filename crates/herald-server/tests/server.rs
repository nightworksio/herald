//! `herald server` over real connections: health, the limits it holds a
//! request to, the work a webhook leaves, its rounds, and stopping.

#[cfg(test)]
#[allow(
    clippy::unused_async_trait_impl,
    reason = "a fake answers at once, and is written as the async trait it fakes"
)]
mod server {
    use std::cell::{Cell, RefCell};
    use std::io;
    use std::net::SocketAddr;
    use std::time::Duration;

    use herald_config::{Config, load};
    use herald_core::{DestinationName, Scheme, Subscription};
    use herald_flows::{Connected, Keys, Ports, Serving, Webhooks, prepare, webhooks};
    use herald_ports::{Admitted, Clock as _, Http, HttpError, Request, Response, Store as _};
    use herald_records::MemoryKeyring;
    use herald_server::{Capped, Heard, Limits, Listener, serve};
    use herald_testing::{FakeClock, FakeSecrets, FakeSigner, FakeStore, FakeVerifier};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::Notify;

    const EVENT: &str =
        include_str!("../../herald-sources-forge/fixtures/github/release-event.reference.json");

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
            }
        }

        async fn serve<L: Listener>(
            &self,
            listener: L,
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
            };
            serve(&serving, listener, limits, heard, stop).await;
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
        }
    }

    async fn listening() -> (TcpListener, SocketAddr) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
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
        let lines: String = headers
            .iter()
            .map(|(name, value)| format!("{name}: {value}\r\n"))
            .collect();
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

    #[tokio::test(start_paused = true)]
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
        let (answer, ()) = tokio::join!(
            client,
            world.serve(listener, limits(), &|_| {}, stop.notified())
        );
        assert!(answer.starts_with("HTTP/1.1 408 "), "{answer}");
    }

    #[tokio::test]
    async fn a_request_that_could_leave_work_is_refused_while_the_queue_is_full() {
        let world = World::new(Gate::default()).await;
        let (listener, address) = listening().await;
        let stop = Notify::new();
        let client = async {
            let answer = asked(address, &posted("/inbox", &[], b"{}")).await;
            stop.notify_one();
            answer
        };
        let none = Limits {
            jobs: 0,
            ..limits()
        };
        let (answer, ()) = tokio::join!(
            client,
            world.serve(listener, none, &|_| {}, stop.notified())
        );
        assert!(answer.starts_with("HTTP/1.1 503 "), "{answer}");
        assert!(answer.contains("retry-after: 30"), "{answer}");
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
            gate.open.notify_one();
            (first, second)
        };
        let one = Limits {
            jobs: 1,
            ..limits()
        };
        let hear = |said: Heard| heard.borrow_mut().push(named(&said));
        let ((first, second), ()) =
            tokio::join!(client, world.serve(listener, one, &hear, stop.notified()));
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
        let seen = Cell::new((false, false));
        world
            .serve(
                listener,
                rounds,
                &|said| {
                    let name = named(&said);
                    let (polled, kept) = seen.get();
                    seen.set((polled || name == "polled", kept || name == "kept"));
                    if seen.get() == (true, true) {
                        stop.notify_one();
                    }
                    heard.borrow_mut().push(name);
                },
                stop.notified(),
            )
            .await;
        assert!(said(&heard).contains(&"polled".to_owned()));
        assert!(said(&heard).contains(&"kept".to_owned()));
    }

    /// A listener whose first connection cannot be taken.
    struct Flaky {
        inner: TcpListener,
        failed: Cell<bool>,
    }

    impl Listener for Flaky {
        type Stream = TcpStream;

        async fn take(&self) -> io::Result<TcpStream> {
            if !self.failed.replace(true) {
                return Err(io::Error::other("too many open files"));
            }
            self.inner.take().await
        }

        fn address(&self) -> io::Result<SocketAddr> {
            Err(io::Error::other("no address"))
        }
    }

    #[tokio::test]
    async fn a_connection_that_cannot_be_taken_is_said_and_the_next_is_taken() {
        let world = World::new(Gate::default()).await;
        let (inner, address) = listening().await;
        let flaky = Flaky {
            inner,
            failed: Cell::new(false),
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

    #[tokio::test]
    async fn a_capped_http_sends_as_many_at_once_as_it_is_allowed() {
        let capped = Capped::new(Gate::default(), 1);
        let request = || Request {
            method: herald_ports::Method::Get,
            url: herald_ports::Text::plain("https://social.example/users/ana"),
            headers: Vec::new(),
            body: Vec::new(),
            most: herald_ports::BODY_LIMIT,
        };
        let (first, second) = tokio::join!(capped.send(request()), capped.send(request()));
        assert_eq!(first.unwrap().status, 202);
        assert_eq!(second.unwrap().status, 202);
    }
}
