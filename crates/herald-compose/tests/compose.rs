//! herald composed on a host made of fakes: every store kind reached
//! through the host's ports, and requests and rounds served over it.

#[cfg(test)]
mod compose {
    use std::collections::BTreeMap;
    use std::future::{Future, ready};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};

    use herald_compose::{
        Composed, Host, HostRequest, KvAt, Left, Opener, ServingOn, SqlAt, Through, reached,
    };
    use herald_config::{Config, Uses, load};
    use herald_core::DestinationName;
    use herald_flows::{Admission, Did, Housekept, Inbound, Job, Stop, Sweep, Taker, Worked};
    use herald_kinds::{Opening, StoreKind};
    use herald_ports::{Changed, Frame, Http, HttpError, Outbound, Request, Response, Value};
    use herald_testing::{
        FakeClock, FakeKv, FakeSecrets, FakeSigner, FakeTimer, FakeVerifier, ScriptedSockets,
        ScriptedSql,
    };

    fn every() -> Vec<&'static str> {
        herald_compose::kinds().store_kinds()
    }

    const NODE: herald_compose::Hosting = herald_compose::Hosting::Node;

    #[derive(Debug, Clone, Default)]
    struct Answering;

    /// A future that is pending once, so that another may run before it
    /// is done.
    struct Yielding(bool);

    impl Future for Yielding {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if self.0 {
                Poll::Ready(())
            } else {
                self.0 = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    impl Http for Answering {
        async fn send(&self, _: Request) -> Result<Response, HttpError> {
            Yielding(false).await;
            Ok(Response {
                status: 200,
                headers: Vec::new(),
                body: b"{}".to_vec(),
            })
        }
    }

    struct Hosting {
        vars: BTreeMap<String, String>,
        secrets: FakeSecrets,
        clock: FakeClock,
        asked: Mutex<Vec<String>>,
        refusing: bool,
        unreached: bool,
        unlaid: bool,
    }

    impl Default for Hosting {
        fn default() -> Self {
            Self {
                vars: BTreeMap::new(),
                secrets: FakeSecrets::default().with("HERALD_STORE_KEY", "k"),
                clock: FakeClock::at(jiff::Timestamp::UNIX_EPOCH),
                asked: Mutex::default(),
                refusing: false,
                unreached: false,
                unlaid: false,
            }
        }
    }

    impl Hosting {
        fn asked(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }

        fn ask(&self, at: String) -> Result<(), String> {
            self.asked.lock().unwrap().push(at);
            if self.refusing {
                Err("refused".to_owned())
            } else {
                Ok(())
            }
        }
    }

    fn opening() -> ScriptedSql {
        ScriptedSql::default().answering(Ok(vec![
            Changed::default(),
            Changed::default(),
            Changed::default(),
            Changed {
                rows: vec![vec![Value::Integer(herald_records::LAYOUT)]],
                changed: 0,
            },
        ]))
    }

    impl Host for Hosting {
        type Http = Answering;
        type Secrets = FakeSecrets;
        type Clock = FakeClock;
        type Signer = FakeSigner;
        type Verifier = FakeVerifier;
        type Sql = ScriptedSql;
        type Kv = FakeKv;

        fn connect(&self, base: &str, _: &Outbound) -> Result<Answering, String> {
            if self.unreached {
                Err(format!("{base} is out of reach"))
            } else {
                Ok(Answering)
            }
        }

        fn var(&self, name: &str) -> Option<&str> {
            self.vars.get(name).map(String::as_str)
        }

        fn secrets(&self) -> &FakeSecrets {
            &self.secrets
        }

        fn clock(&self) -> &FakeClock {
            &self.clock
        }

        fn signer(&self) -> &FakeSigner {
            &FakeSigner
        }

        fn verifier(&self) -> &FakeVerifier {
            &FakeVerifier
        }

        fn sql(&self, at: SqlAt<'_>) -> impl Future<Output = Result<ScriptedSql, String>> {
            ready(self.ask(format!("{at:?}")).map(|()| {
                if self.unlaid {
                    ScriptedSql::default()
                } else {
                    opening()
                }
            }))
        }

        fn kv(&self, at: KvAt<'_>) -> impl Future<Output = Result<FakeKv, String>> {
            ready(self.ask(format!("{at:?}")).map(|()| FakeKv::default()))
        }

        fn nonce(&self) -> String {
            static NONCES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            NONCES
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                .to_string()
        }
    }

    struct Untaken;

    impl Taker for Untaken {
        type Place = ();

        fn place(&self) -> Option<()> {
            None
        }

        fn fill(&self, (): (), _: Job) {}

        fn report(&self, _: &DestinationName, _: String) {}
    }

    /// The store table `store`, with the key a store in a shared repository
    /// seals its records with.
    fn sealed(store: &str) -> String {
        let shared = ["git-branch", "repo-variables", "actions-cache"]
            .iter()
            .any(|kind| store.starts_with(&format!("kind = \"{kind}\"")));
        if shared {
            format!("{store}\nkey = {{ secret = \"HERALD_STORE_KEY\" }}")
        } else {
            store.to_owned()
        }
    }

    fn config(store: &str) -> Config {
        let text = format!("version = 1\n[store]\n{}\n", sealed(store));
        load(
            "herald.toml",
            &text,
            &|_| Err(String::new()),
            &herald_compose::kinds(),
        )
        .unwrap()
    }

    fn opener(host: &Arc<Hosting>) -> Arc<Opener<Hosting>> {
        Arc::new(Opener::new(Arc::clone(host), Outbound::default()))
    }

    fn kept(host: &Arc<Hosting>, store: &str) -> Result<&'static str, String> {
        let config = config(store);
        let records = pollster::block_on(reached(
            opener(host),
            &config.store,
            config.attempts,
            &every(),
        ))?;
        Ok(if records.reads_posts() {
            "posts"
        } else {
            "kept"
        })
    }

    fn github() -> Hosting {
        Hosting {
            vars: BTreeMap::from([("GITHUB_REPOSITORY".to_owned(), "cli/cli".to_owned())]),
            secrets: FakeSecrets::default()
                .with("HERALD_STORE_KEY", "k")
                .with("GITHUB_TOKEN", "t")
                .with("VARIABLES", "v")
                .with("DATABASE_URL", "postgres://db")
                .with("AWS_ACCESS_KEY_ID", "id")
                .with("AWS_SECRET_ACCESS_KEY", "key")
                .with("AWS_REGION", "eu-west-1"),
            ..Hosting::default()
        }
    }

    #[test]
    fn an_opener_reads_its_host_s_variables_and_takes_its_nonces() {
        let host = Arc::new(github());
        let opener = opener(&host);
        assert_eq!(opener.var("GITHUB_REPOSITORY"), Some("cli/cli"));
        let nonce = opener.nonce();
        assert!(nonce.parse::<u64>().is_ok(), "{nonce}");
        assert_ne!(opener.nonce(), nonce);
    }

    #[test]
    fn each_store_is_reached_through_what_the_host_offers() {
        let host = Arc::new(github());
        let kinds = [
            ("kind = \"sqlite\"", "kept"),
            (
                "kind = \"postgres\"\nurl = { secret = \"DATABASE_URL\" }",
                "kept",
            ),
            ("kind = \"d1\"\nbinding = \"DB\"", "kept"),
            ("kind = \"durable-object\"", "kept"),
            ("kind = \"deno-kv\"\npath = \"/data/kv\"", "kept"),
            ("kind = \"fastly-kv\"", "kept"),
            ("kind = \"git-branch\"", "kept"),
            (
                "kind = \"repo-variables\"\ntoken = { secret = \"VARIABLES\" }",
                "kept",
            ),
            ("kind = \"dynamodb\"", "kept"),
            ("kind = \"markers\"", "posts"),
        ];
        for (store, records) in kinds {
            assert_eq!(kept(&host, store), Ok(records), "{store}");
        }
        assert_eq!(
            host.asked(),
            [
                "Sqlite { file: \"herald.sqlite\", busy: 5s }",
                "Postgres { url: «secret:DATABASE_URL», connect: 10s, statement: 30s }",
                "D1(\"DB\")",
                "DurableObject { binding: \"HERALD\", object: \"herald\" }",
                "DenoKv(Some(\"/data/kv\"))",
                "FastlyKv { store: \"herald\", tombstone: 1h }",
            ]
        );
    }

    #[test]
    fn a_store_the_host_cannot_open_or_reach_says_which_and_why() {
        let refusing = Arc::new(Hosting {
            refusing: true,
            ..github()
        });
        assert_eq!(
            kept(&refusing, "kind = \"sqlite\"\npath = \"kept.sqlite\""),
            Err("the sqlite store kept.sqlite could not be opened: refused".to_owned())
        );
        for kind in ["d1", "durable-object", "deno-kv", "fastly-kv"] {
            assert_eq!(
                kept(&refusing, &format!("kind = \"{kind}\"")),
                Err(format!("the {kind} store could not be opened: refused"))
            );
        }
        assert_eq!(
            kept(
                &refusing,
                "kind = \"postgres\"\nurl = { secret = \"DATABASE_URL\" }"
            ),
            Err("the postgres store could not be opened: refused".to_owned())
        );
        let unreached = Arc::new(Hosting {
            unreached: true,
            ..github()
        });
        assert_eq!(
            kept(&unreached, "kind = \"git-branch\""),
            Err("https://api.github.com is out of reach".to_owned())
        );
        let keyless = Arc::new(Hosting {
            secrets: FakeSecrets::default().with("GITHUB_TOKEN", "t"),
            ..github()
        });
        assert_eq!(
            kept(&keyless, "kind = \"git-branch\""),
            Err("HERALD_STORE_KEY is not set; the store seals its records with it".to_owned())
        );
        let bare = Arc::new(Hosting::default());
        assert_eq!(
            kept(
                &bare,
                "kind = \"postgres\"\nurl = { secret = \"DATABASE_URL\" }"
            ),
            Err(
                "DATABASE_URL is not set; the postgres store reads its connection URL from it"
                    .to_owned()
            )
        );
        assert_eq!(
            kept(&bare, "kind = \"git-branch\"\nrepository = \"cli/cli\""),
            Err(
                "GITHUB_TOKEN is not set; the git-branch store needs a token that may push"
                    .to_owned()
            )
        );
        assert_eq!(
            kept(&bare, "kind = \"repo-variables\"\ntoken = { secret = \"VARIABLES\" }"),
            Err("GITHUB_REPOSITORY is not set; name the store's repository, or run herald in the repository's CI".to_owned())
        );
    }

    #[test]
    fn a_database_whose_tables_cannot_be_laid_out_opens_no_store() {
        let host = Arc::new(Hosting {
            unlaid: true,
            ..github()
        });
        let laid = "could not be opened: the store could not be reached: the database refused the statements: no answer was scripted";
        for store in [
            "kind = \"postgres\"\nurl = { secret = \"DATABASE_URL\" }",
            "kind = \"d1\"",
            "kind = \"durable-object\"",
        ] {
            let refused = kept(&host, store).unwrap_err();
            assert!(refused.ends_with(laid), "{refused}");
        }
        assert!(
            kept(&host, "kind = \"sqlite\"")
                .unwrap_err()
                .starts_with("the sqlite store herald.sqlite could not be opened: ")
        );
    }

    #[test]
    fn a_kind_the_host_does_not_keep_records_in_is_refused_by_name() {
        let host = Arc::new(github());
        let d1 = config("kind = \"d1\"");
        let refused = pollster::block_on(reached(
            opener(&host),
            &d1.store,
            d1.attempts,
            &[herald_store_markers::MarkersKind::NAME],
        ));
        assert_eq!(
            refused.err(),
            Some(
                "this herald keeps records in markers; set [store] kind to one of them".to_owned()
            )
        );
        let cached = config("kind = \"actions-cache\"");
        let unkept = pollster::block_on(reached(
            opener(&host),
            &cached.store,
            cached.attempts,
            &every(),
        ));
        assert_eq!(
            unkept.err(),
            Some("ACTIONS_RESULTS_URL is not set; herald reads it from the CI run".to_owned())
        );
        assert_eq!(host.asked(), Vec::<String>::new());
    }

    #[test]
    fn a_composed_herald_answers_polls_and_keeps_house_with_keys_kept_either_way() {
        for keys in ["memory", "store"] {
            let text = format!(
                "version = 1\n[store]\nkind = \"deno-kv\"\n[server.keys]\ncache = \"{keys}\"\n"
            );
            let config = load(
                "herald.toml",
                &text,
                &|_| Err(String::new()),
                &herald_compose::kinds(),
            )
            .unwrap();
            let composed = pollster::block_on(Composed::new(
                Hosting::default(),
                config,
                NODE,
                Uses::Serves,
                "test",
            ))
            .unwrap();
            assert_eq!(
                composed.config().store.kind(),
                herald_store_deno_kv::DenoKvKind::NAME
            );
            assert_eq!(composed.host().asked(), ["DenoKv(None)"]);
            let health = Inbound {
                method: "GET",
                path: "/healthz",
                query: None,
                headers: &[],
                body: &[],
            };
            let answered = pollster::block_on(composed.answer(&health, Admission::Once));
            assert_eq!(answered.reply.status, 200);
            assert!(answered.left.is_none());
            assert_eq!(pollster::block_on(composed.poll()), Vec::new());
            assert_eq!(
                pollster::block_on(composed.housekeep()),
                Ok(Housekept::default())
            );
            let sockets = ScriptedSockets::default();
            pollster::block_on(composed.hold(&sockets, &Untaken, &FakeTimer::default()));
            assert_eq!(sockets.opened(), Vec::<String>::new());
            let destination = DestinationName::new("chat");
            assert_eq!(Untaken.place(), None);
            Untaken.fill(
                (),
                Job::Interact {
                    destination: destination.clone(),
                    asked: Vec::new(),
                },
            );
            Untaken.report(&destination, String::new());
        }
    }

    #[test]
    fn a_composed_herald_polls_its_sources_stops_keeping_house_and_holds_its_sessions() {
        for keys in ["memory", "store"] {
            let text = format!(
                r#"version = 1
[store]
kind = "sqlite"
[server.keys]
cache = "{keys}"
[sources.gate]
kind = "github"
repository = "cli/cli"
[templates.t]
title = "t"
body = "b"
[destinations.chat]
kind = "discord"
token = {{ secret = "TOKEN" }}
[destinations.chat.interactions]
application = "123456789012345678"
via = "gateway"
[[routes]]
id = "announced"
sources = ["gate"]
destination = "chat"
channel = "323456789012345678"
template = "t"
"#
            );
            let config = load(
                "herald.toml",
                &text,
                &|_| Err(String::new()),
                &herald_compose::kinds(),
            )
            .unwrap();
            let host = Hosting {
                secrets: FakeSecrets::default().with("TOKEN", "t"),
                ..Hosting::default()
            };
            let composed =
                pollster::block_on(Composed::new(host, config, NODE, Uses::Serves, "test"))
                    .unwrap();
            let swept = pollster::block_on(composed.poll());
            let unscripted = || {
                Stop::Store(
                    "the store could not be reached: the database refused the statements: no answer was scripted"
                        .to_owned(),
                )
            };
            assert_eq!(swept.len(), 1);
            assert_eq!(swept[0].source.as_str(), "gate");
            assert_eq!(swept[0].outcome, Sweep::Stopped(unscripted()));
            assert_eq!(pollster::block_on(composed.housekeep()), Err(unscripted()));
            let sockets = ScriptedSockets::opening(vec![Ok(vec![Frame::Closed(Some(4004))])]);
            pollster::block_on(composed.hold(&sockets, &Untaken, &FakeTimer::default()));
            assert_eq!(sockets.opened().len(), 1);
        }
    }

    #[test]
    fn a_round_asked_while_one_of_its_kind_runs_does_nothing() {
        let text = "version = 1\n[store]\nkind = \"git-branch\"\nkey = { secret = \"HERALD_STORE_KEY\" }\n[sources.gate]\nkind = \"github\"\nrepository = \"cli/cli\"\n";
        let config = load(
            "herald.toml",
            text,
            &|_| Err(String::new()),
            &herald_compose::kinds(),
        )
        .unwrap();
        let composed =
            pollster::block_on(Composed::new(github(), config, NODE, Uses::Serves, "test"))
                .unwrap();
        let (swept, unswept) =
            pollster::block_on(futures_util::future::join(composed.poll(), composed.poll()));
        assert_eq!(swept.len(), 1);
        assert_eq!(unswept, Vec::new());
        let (kept, unkept) = pollster::block_on(futures_util::future::join(
            composed.housekeep(),
            composed.housekeep(),
        ));
        assert_ne!(kept, Ok(Housekept::default()));
        assert_eq!(unkept, Ok(Housekept::default()));
        assert_eq!(pollster::block_on(composed.poll()).len(), 1);
    }

    #[test]
    fn a_composed_herald_works_over_every_store_with_keys_kept_either_way() {
        let stores = [
            "kind = \"sqlite\"",
            "kind = \"deno-kv\"",
            "kind = \"git-branch\"",
            "kind = \"repo-variables\"\ntoken = { secret = \"VARIABLES\" }",
            "kind = \"dynamodb\"",
            "kind = \"markers\"",
        ];
        let mut housekeeping = Vec::new();
        for store in stores {
            let kept: &[&str] = if store.contains("markers") {
                &["memory"]
            } else {
                &["memory", "store"]
            };
            for keys in kept {
                let text = format!(
                    "version = 1\n[store]\n{}\n[server.keys]\ncache = \"{keys}\"\n",
                    sealed(store)
                );
                let config = load(
                    "herald.toml",
                    &text,
                    &|_| Err(String::new()),
                    &herald_compose::kinds(),
                )
                .unwrap();
                let composed =
                    pollster::block_on(Composed::new(github(), config, NODE, Uses::Serves, "test"))
                        .unwrap();
                let health = Inbound {
                    method: "GET",
                    path: "/healthz",
                    query: None,
                    headers: &[],
                    body: &[],
                };
                let answered = pollster::block_on(composed.answer(&health, Admission::Once));
                assert_eq!(answered.reply.status, 200, "{store}");
                assert_eq!(pollster::block_on(composed.poll()), Vec::new(), "{store}");
                let housekept = pollster::block_on(composed.housekeep());
                housekeeping.push(format!("{store} {keys} {housekept:?}"));
                let sockets = ScriptedSockets::default();
                pollster::block_on(composed.hold(&sockets, &Untaken, &FakeTimer::default()));
                let job = Job::Interact {
                    destination: DestinationName::new("chat"),
                    asked: Vec::new(),
                };
                let place = composed.place().expect("a place in an empty room");
                assert_eq!(
                    pollster::block_on(composed.work(Left::new(job, place))),
                    Worked {
                        did: Did::Interacted(Vec::new()),
                        unreleased: None,
                    },
                    "{store}"
                );
            }
        }
        let unscripted = "Err(Store(\"the store could not be reached: the database refused the statements: no answer was scripted\"))";
        let untreed = "Err(Store(\"a record in the store could not be read: deliveries: missing field `tree` at line 1 column 2\"))";
        let uncounted = "Err(Store(\"a record in the store could not be read: the repository's variables: missing field `total_count` at line 1 column 2\"))";
        let kept = "Ok(Housekept { kept: [], skipped: [] })";
        let variables = "kind = \"repo-variables\"\ntoken = { secret = \"VARIABLES\" }";
        assert_eq!(
            housekeeping,
            [
                format!("kind = \"sqlite\" memory {unscripted}"),
                format!("kind = \"sqlite\" store {unscripted}"),
                format!("kind = \"deno-kv\" memory {kept}"),
                format!("kind = \"deno-kv\" store {kept}"),
                format!("kind = \"git-branch\" memory {untreed}"),
                format!(
                    "kind = \"git-branch\" store {}",
                    untreed.replace("deliveries", "keys")
                ),
                format!("{variables} memory {uncounted}"),
                format!("{variables} store {uncounted}"),
                format!("kind = \"dynamodb\" memory {kept}"),
                format!("kind = \"dynamodb\" store {kept}"),
                format!("kind = \"markers\" memory {kept}"),
            ]
        );
    }

    fn asked(method: &str, target: &str, authorization: Option<&str>) -> HostRequest {
        HostRequest {
            method: method.to_owned(),
            target: target.to_owned(),
            headers: authorization
                .map(|value| ("Authorization".to_owned(), value.to_owned()))
                .into_iter()
                .collect(),
            body: Vec::new(),
        }
    }

    #[test]
    fn a_request_is_answered_or_runs_the_round_it_holds_the_secret_for() {
        let unset = pollster::block_on(Composed::new(
            Hosting::default(),
            config("kind = \"deno-kv\""),
            NODE,
            Uses::Serves,
            "test",
        ))
        .unwrap();
        let reach = |composed: &Composed<'_, Hosting>, asked: &HostRequest| {
            pollster::block_on(composed.reached(asked))
        };
        let health = reach(&unset, &asked("GET", "/healthz?x=1", None));
        assert_eq!(health.reply.status, 200);
        assert_eq!(health.said, Vec::<String>::new());
        let round = asked("POST", "/herald/round/poll", Some("Bearer s3cret"));
        assert_eq!(reach(&unset, &round).reply.status, 401);
        let host = Hosting {
            secrets: FakeSecrets::default().with("HERALD_ROUND_SECRET", "s3cret"),
            ..Hosting::default()
        };
        let composed = pollster::block_on(Composed::new(
            host,
            config("kind = \"deno-kv\""),
            NODE,
            Uses::Serves,
            "test",
        ))
        .unwrap();
        let unheld = reach(&composed, &asked("POST", "/herald/round/poll", None));
        assert_eq!(
            (unheld.reply.status, unheld.refused.as_deref()),
            (401, Some(herald_flows::round::UNASKED))
        );
        for round in ["poll", "housekeep"] {
            let held = reach(
                &composed,
                &asked(
                    "POST",
                    &format!("/herald/round/{round}"),
                    Some("Bearer s3cret"),
                ),
            );
            assert_eq!(held.reply.status, 200, "{round}");
            assert_eq!(held.reply.body, b"the round ran");
            assert!(held.left.is_none());
        }
    }

    #[test]
    fn a_request_carries_its_headers_to_the_answer() {
        let text = r#"version = 1
[store]
kind = "deno-kv"
[destinations.fedi]
kind = "activitypub"
domain = "herald.example"
key = { secret = "KEY" }
[sources.gate]
kind = "github"
repository = "cli/cli"
[[routes]]
id = "followed"
sources = ["gate"]
destination = "fedi"
template = "t"
actor = "herald"
[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
"#;
        let config = load(
            "herald.toml",
            text,
            &|_| Err(String::new()),
            &herald_compose::kinds(),
        )
        .unwrap();
        let host = Hosting {
            secrets: FakeSecrets::default().with("KEY", "k"),
            ..Hosting::default()
        };
        let composed =
            pollster::block_on(Composed::new(host, config, NODE, Uses::Serves, "test")).unwrap();
        let mut inbox = asked("POST", "/inbox", None);
        let unaddressed = pollster::block_on(composed.reached(&inbox));
        assert_eq!(unaddressed.reply.status, 404);
        inbox
            .headers
            .push(("host".to_owned(), "herald.example".to_owned()));
        let addressed = pollster::block_on(composed.reached(&inbox));
        assert_eq!(addressed.reply.status, 401);
    }

    #[test]
    fn a_body_or_work_past_the_server_s_limits_is_refused_and_its_delivery_released() {
        let text = format!(
            r#"version = 1
[store]
kind = "deno-kv"
[server]
jobs = 1
body = "1KiB"
[destinations.chat]
kind = "slack"
token = {{ secret = "TOKEN" }}
[[routes]]
id = "deployed"
sources = ["ci"]
destination = "chat"
channel = "C0123456789"
template = "t"
[apps.ci]
tokens = ["sha256:{}"]
projects = ["herald"]
routes = ["deployed"]
[templates.t]
title = "{{{{ event.title }}}}"
body = "{{{{ event.body }}}}"
"#,
            herald_core::digest("app-token")
        );
        let config = load(
            "herald.toml",
            &text,
            &|_| Err(String::new()),
            &herald_compose::kinds(),
        )
        .unwrap();
        let host = Hosting {
            secrets: FakeSecrets::default().with("TOKEN", "t"),
            ..Hosting::default()
        };
        let composed =
            pollster::block_on(Composed::new(host, config, NODE, Uses::Serves, "test")).unwrap();
        let headers = [("Authorization", "Bearer app-token")];
        let event =
            |id: &str| format!(r#"{{"id": "{id}", "project": "herald", "title": "Deployed"}}"#);
        let post = |body: &str| {
            let inbound = Inbound {
                method: "POST",
                path: "/api/projects/herald/events",
                query: None,
                headers: &headers,
                body: body.as_bytes(),
            };
            pollster::block_on(composed.answer(&inbound, Admission::Once))
        };
        let large = post(&"0".repeat(1025));
        assert_eq!(
            (large.reply.status, large.reply.body.as_slice()),
            (413, b"herald does not take this request".as_slice())
        );
        let most = post(&format!("{:<1024}", event("0")));
        assert_eq!(most.reply.status, 202);
        drop(most);
        let first = post(&event("1"));
        assert_eq!(first.reply.status, 202);
        assert!(first.left.is_some());
        let busy = post(&event("2"));
        assert_eq!(
            (
                busy.reply.status,
                busy.reply.retry_after,
                busy.left.is_none()
            ),
            (503, Some(30), true)
        );
        drop(first);
        let again = post(&event("2"));
        assert_eq!(again.reply.status, 202);
    }

    #[test]
    fn a_herald_missing_a_secret_is_not_composed_and_says_which() {
        const CHAT: &str =
            "[destinations.chat]\nkind = \"slack\"\ntoken = { secret = \"TOKEN\" }\n";
        for (rest, missing) in [
            (CHAT.to_owned(), "TOKEN"),
            (
                format!(
                    "{CHAT}[destinations.chat.interactions]\nsigning = {{ secret = \"SIGNING\" }}\n"
                ),
                "SIGNING",
            ),
            (
                "[sources.gate]\nkind = \"github\"\nrepository = \"cli/cli\"\nwebhook = { secret = \"HOOK\" }\n"
                    .to_owned(),
                "HOOK",
            ),
        ] {
            let text = format!("version = 1\n[store]\nkind = \"deno-kv\"\n{rest}");
            let config = load("herald.toml", &text, &|_| Err(String::new()), &herald_compose::kinds()).unwrap();
            let host = Hosting {
                secrets: if missing == "TOKEN" {
                    FakeSecrets::default()
                } else {
                    FakeSecrets::default().with("TOKEN", "t")
                },
                ..Hosting::default()
            };
            let refused = pollster::block_on(Composed::new(host, config, NODE, Uses::Serves, "test")).err();
            assert!(
                refused.as_deref().is_some_and(|said| said.contains(missing)),
                "{missing}: {refused:?}"
            );
        }
    }

    #[test]
    fn a_herald_whose_store_cannot_be_reached_is_not_composed() {
        let unreached = Hosting {
            unreached: true,
            ..Hosting::default()
        };
        let composed = pollster::block_on(Composed::new(
            unreached,
            config("kind = \"markers\""),
            NODE,
            Uses::Serves,
            "test",
        ));
        assert_eq!(composed.err(), Some("public: is out of reach".to_owned()));
    }

    struct Named;

    impl<H: Host> Through<H> for Named {
        type Done = (String, bool);

        fn through<S: herald_ports::Store, K: herald_ports::Keyring>(
            self,
            serving: &ServingOn<'_, H, S, K>,
            run: &str,
        ) -> impl Future<Output = (String, bool)> {
            ready((run.to_owned(), serving.ports.store.cursors().is_some()))
        }
    }

    #[test]
    fn a_dry_herald_keeps_its_records_in_memory_and_opens_no_store() {
        let composed = pollster::block_on(Composed::dry(
            Hosting::default(),
            config("kind = \"deno-kv\""),
            Uses::Announces,
            "dry",
        ))
        .unwrap();
        assert_eq!(composed.host().asked(), Vec::<String>::new());
        assert!(
            composed
                .connected()
                .interacting(&DestinationName::new("chat"))
                .is_none()
        );
        let (run, kept) = pollster::block_on(composed.run(Named));
        assert!(run.starts_with("dry/"), "{run}");
        assert!(run.len() > "dry/".len(), "{run}");
        assert!(kept);
        let (again, _) = pollster::block_on(composed.run(Named));
        assert_ne!(again, run);
    }

    #[test]
    fn a_herald_reads_only_the_secrets_its_run_uses() {
        let text = "version = 1\n[store]\nkind = \"markers\"\n[sources.app]\nkind = \"github\"\nrepository = \"o/r\"\ntoken = { secret = \"SOURCE_TOKEN\" }\n";
        let config = load(
            "herald.toml",
            text,
            &|_| Err(String::new()),
            &herald_compose::kinds(),
        )
        .unwrap();
        let announcing = pollster::block_on(Composed::dry(
            Hosting::default(),
            config.clone(),
            Uses::Announces,
            "dry",
        ));
        assert!(announcing.is_ok());
        let polling = pollster::block_on(Composed::dry(
            Hosting::default(),
            config,
            Uses::Polls,
            "dry",
        ));
        assert!(
            polling
                .err()
                .is_some_and(|error| error.contains("SOURCE_TOKEN"))
        );
    }
}
