use herald_config::{Config, load};
use herald_core::{Key, RouteId, Scheme, Step};
use herald_flows::{
    Answered, Connected, FollowError, Followed, Inbound, Job, Keys, Outcome, Ports, Serving,
    Webhooks, Worked, answer, prepare, unworked, webhooks, work,
};
use herald_ports::{Admitted, Algorithm, CryptoError, Secret, Signer, Store as _};
use herald_records::MemoryKeyring;
use herald_testing::{FakeClock, FakeSecrets, FakeSigner, FakeVerifier};

use crate::harness::{Failing, Fault, Script, healthy};
use crate::inbound::{ANA, ANA_KEY, FOLLOW, activity, at, borrowed, document};

const EVENT: &str =
    include_str!("../../../herald-sources-forge/fixtures/github/release-event.reference.json");
const ACTOR: &str = "https://herald.example/users/php-mutation-gate";

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
[sources.quiet]
kind = "github"
repository = "cli/cli"
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

struct World {
    config: Config,
    connected: Connected<Script>,
    webhooks: Webhooks,
    store: Failing,
    keyring: MemoryKeyring,
    script: Script,
}

type Served<'a> =
    Serving<'a, Failing, FakeClock, FakeSigner, FakeVerifier, Script, Script, MemoryKeyring>;

impl World {
    fn new(script: Script) -> Self {
        let config = config();
        let prepared = pollster::block_on(prepare(&config, &secrets(), &FakeSigner)).unwrap();
        let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
        let webhooks = pollster::block_on(webhooks(&config, &secrets())).unwrap();
        Self {
            config,
            connected,
            webhooks,
            store: healthy(),
            keyring: MemoryKeyring::new(8),
            script,
        }
    }

    fn failing(fault: Fault) -> Self {
        let mut world = Self::new(Script::default());
        world.store.fault = fault;
        world
    }

    fn serving<T>(&self, act: impl FnOnce(&Served<'_>) -> T) -> T {
        let clock = FakeClock::at(at());
        let ports = Ports {
            store: &self.store,
            clock: &clock,
            signer: &FakeSigner,
        };
        let keys = Keys {
            keyring: &self.keyring,
            refreshed: jiff::SignedDuration::from_hours(1),
        };
        act(&Serving {
            config: &self.config,
            connected: &self.connected,
            ports: &ports,
            verifier: &FakeVerifier,
            public: &self.script,
            keys: &keys,
            webhooks: &self.webhooks,
        })
    }

    fn answer(&self, inbound: &Inbound<'_>) -> Answered {
        self.serving(|serving| pollster::block_on(answer(serving, inbound)))
    }

    fn work(&self, job: Job) -> Worked {
        self.serving(|serving| pollster::block_on(work(serving, job, "server")))
    }

    fn admits(&self, scheme: Scheme, delivery: &str) -> Admitted {
        pollster::block_on(self.store.inner.admit(scheme, delivery, at())).unwrap()
    }
}

fn get(path: &str) -> Inbound<'_> {
    Inbound {
        method: "GET",
        path,
        query: None,
        headers: &[],
        body: &[],
    }
}

fn said(answered: &Answered) -> (u16, &'static str, String) {
    (
        answered.reply.status,
        answered.reply.kind,
        String::from_utf8(answered.reply.body.clone()).unwrap(),
    )
}

const TEXT: &str = "text/plain; charset=utf-8";

/// A webhook's path, its headers and the status it is answered with.
type Case<'a> = (&'a str, &'a [(&'a str, &'a str)], u16);
const JSON: &str = "application/activity+json";

#[test]
fn health_is_answered_and_every_path_herald_does_not_serve_is_not_found() {
    let world = World::new(Script::default());
    assert_eq!(
        said(&world.answer(&get("/healthz"))),
        (200, TEXT, "herald is up".to_owned())
    );
    let posted = Inbound {
        method: "POST",
        ..get("/healthz")
    };
    assert_eq!(world.answer(&posted).reply.status, 405);
    for path in [
        "/",
        "/users/nobody",
        "/users/php-mutation-gate/outbox",
        "/users/php-mutation-gate/notes",
    ] {
        assert_eq!(world.answer(&get(path)).reply.status, 404, "{path}");
    }
    for (method, path) in [
        ("PUT", "/users/php-mutation-gate"),
        ("GET", "/users/php-mutation-gate/inbox"),
        ("POST", "/users/php-mutation-gate/notes/0"),
        ("GET", "/hooks/gate"),
        ("DELETE", "/inbox"),
    ] {
        let asked = Inbound {
            method,
            ..get(path)
        };
        assert_eq!(world.answer(&asked).reply.status, 405, "{method} {path}");
    }
}

#[test]
fn webfinger_names_an_actor_herald_serves_by_its_account_or_its_id() {
    let world = World::new(Script::default());
    for query in [
        "resource=acct%3Aphp-mutation-gate%40herald.example",
        "rel=self&resource=acct:php-mutation-gate@herald.example",
        "resource=https%3A%2F%2Fherald.example%2Fusers%2Fphp-mutation-gate",
    ] {
        let asked = Inbound {
            query: Some(query),
            ..get("/.well-known/webfinger")
        };
        let (status, kind, body) = said(&world.answer(&asked));
        assert_eq!((status, kind), (200, "application/jrd+json"), "{query}");
        let found: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(found["subject"], "acct:php-mutation-gate@herald.example");
        assert_eq!(found["links"][0]["href"], ACTOR);
    }
    for query in [
        None,
        Some("resource=acct:nobody@herald.example"),
        Some("resource=%zz"),
    ] {
        let asked = Inbound {
            query,
            ..get("/.well-known/webfinger")
        };
        assert_eq!(world.answer(&asked).reply.status, 404, "{query:?}");
    }
}

#[test]
fn an_actor_is_served_its_document_with_its_public_key() {
    let world = World::new(Script::default());
    let (status, kind, body) = said(&world.answer(&get("/users/php-mutation-gate")));
    assert_eq!((status, kind), (200, JSON));
    let document: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(document["id"], ACTOR);
    assert_eq!(document["publicKey"]["owner"], ACTOR);
    assert!(
        document["publicKey"]["publicKeyPem"]
            .as_str()
            .unwrap()
            .starts_with("-----BEGIN PUBLIC KEY-----")
    );
}

/// A signer that cannot read any key.
struct Keyless;

impl Signer for Keyless {
    async fn sign(&self, _: Algorithm, key: &Secret, _: &[u8]) -> Result<Vec<u8>, CryptoError> {
        Err(CryptoError::BadKey {
            name: key.name().to_owned(),
        })
    }

    async fn public(&self, _: Algorithm, key: &Secret) -> Result<Vec<u8>, CryptoError> {
        Err(CryptoError::BadKey {
            name: key.name().to_owned(),
        })
    }
}

#[test]
fn an_actor_whose_key_cannot_be_read_is_not_served() {
    let world = World::new(Script::default());
    let clock = FakeClock::at(at());
    let ports = Ports {
        store: &world.store,
        clock: &clock,
        signer: &Keyless,
    };
    let keys = Keys {
        keyring: &world.keyring,
        refreshed: jiff::SignedDuration::from_hours(1),
    };
    let serving = Serving {
        config: &world.config,
        connected: &world.connected,
        ports: &ports,
        verifier: &FakeVerifier,
        public: &world.script,
        keys: &keys,
        webhooks: &world.webhooks,
    };
    let answered = pollster::block_on(answer(&serving, &get("/users/php-mutation-gate")));
    assert_eq!(answered.reply.status, 500);
    assert!(
        String::from_utf8(answered.reply.body)
            .unwrap()
            .starts_with("the actor's key cannot be read:")
    );
    assert!(
        pollster::block_on(Keyless.sign(Algorithm::RsaSha256, &Secret::new("K", "v"), b""))
            .is_err()
    );
}

fn key() -> Key {
    let event = herald_sources_forge::Forge::GitHub
        .event(EVENT.as_bytes())
        .unwrap()
        .unwrap();
    Key::derive(&RouteId::new("followed"), event.identity())
}

#[test]
fn a_note_is_served_by_its_id_where_its_actor_published_it() {
    let world = World::new(Script::default());
    let key = key();
    let path = format!("/users/php-mutation-gate/notes/{key}");
    assert_eq!(world.answer(&get(&path)).reply.status, 404);
    let note = format!(r#"{{"id":"{ACTOR}/notes/{key}","type":"Note"}}"#);
    pollster::block_on(world.store.inner.put_note(&key, &note)).unwrap();
    assert_eq!(said(&world.answer(&get(&path))), (200, JSON, note));
    let other = format!(r#"{{"id":"https://herald.example/users/other/notes/{key}"}}"#);
    pollster::block_on(world.store.inner.put_note(&key, &other)).unwrap();
    assert_eq!(world.answer(&get(&path)).reply.status, 404);
    pollster::block_on(world.store.inner.put_note(&key, "not json")).unwrap();
    assert_eq!(world.answer(&get(&path)).reply.status, 404);
    assert_eq!(
        world
            .answer(&get("/users/php-mutation-gate/notes/not-a-key"))
            .reply
            .status,
        404
    );
    let unread = World::failing(Fault::ReadsNotes);
    assert_eq!(unread.answer(&get(&path)).reply.status, 503);
}

fn signed(secret: &str, body: &[u8]) -> String {
    format!(
        "sha256={}",
        herald_core::hex(&[format!("HmacSha256|{secret}|").as_bytes(), body].concat())
    )
}

fn hooked<'a>(source: &'a str, headers: &'a [(&'a str, &'a str)], body: &'a [u8]) -> Inbound<'a> {
    Inbound {
        method: "POST",
        path: source,
        query: None,
        headers,
        body,
    }
}

#[test]
fn a_signed_webhook_of_a_release_is_answered_and_left_to_announce() {
    let world = World::new(Script::default());
    let signature = signed("hook-secret", EVENT.as_bytes());
    let headers = [
        ("X-Hub-Signature-256", signature.as_str()),
        ("X-GitHub-Event", "release"),
    ];
    let answered = world.answer(&hooked("/hooks/gate", &headers, EVENT.as_bytes()));
    assert_eq!(
        said(&answered),
        (202, TEXT, "herald announces the release".to_owned())
    );
    let Some(Job::Announce {
        source,
        scheme,
        verified,
        ..
    }) = answered.job.clone()
    else {
        panic!("the release is left to announce");
    };
    assert_eq!((source.as_str(), scheme), ("gate", Scheme::GitHub));
    let delivery = verified.delivery.clone().unwrap();
    let again = world.answer(&hooked("/hooks/gate", &headers, EVENT.as_bytes()));
    assert_eq!(again.reply.status, 409);
    let Some(job) = answered.job else {
        panic!("the release is left to announce");
    };
    assert_eq!(
        world.work(job),
        Worked::Announced(vec![herald_flows::Announced {
            route: RouteId::new("followed"),
            outcome: Outcome::Sent {
                steps: vec![Step::Post]
            },
        }])
    );
    assert_eq!(world.admits(Scheme::GitHub, &delivery), Admitted::Seen);
    let note = world.answer(&get(&format!("/users/php-mutation-gate/notes/{}", key())));
    let (status, kind, body) = said(&note);
    assert_eq!((status, kind), (200, JSON));
    let note: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(note["type"], "Note");
    assert_eq!(note["attributedTo"], ACTOR);
}

#[test]
fn a_webhook_that_is_not_let_through_or_announces_nothing_is_answered_by_why() {
    let world = World::new(Script::default());
    let signature = signed("hook-secret", EVENT.as_bytes());
    let forged = signed("guessed", EVENT.as_bytes());
    let cases: [Case<'_>; 4] = [
        ("/hooks/nowhere", &[], 404),
        ("/hooks/quiet", &[], 404),
        (
            "/hooks/gate",
            &[("X-Hub-Signature-256", forged.as_str())],
            401,
        ),
        (
            "/hooks/gate",
            &[
                ("X-Hub-Signature-256", signature.as_str()),
                ("X-GitHub-Event", "ping"),
            ],
            200,
        ),
    ];
    for (path, headers, status) in cases {
        let answered = world.answer(&hooked(path, headers, EVENT.as_bytes()));
        assert_eq!(answered.reply.status, status, "{path} {headers:?}");
        assert_eq!(answered.job, None);
    }
    let unadmitted = World::failing(Fault::Admits);
    let headers = [
        ("X-Hub-Signature-256", signature.as_str()),
        ("X-GitHub-Event", "release"),
    ];
    assert_eq!(
        unadmitted
            .answer(&hooked("/hooks/gate", &headers, EVENT.as_bytes()))
            .reply
            .status,
        503
    );
}

#[test]
fn a_webhook_of_another_repository_is_refused_and_its_delivery_released() {
    let world = World::new(Script::default());
    let elsewhere = EVENT.replace(
        "\"full_name\": \"cli/cli\"",
        "\"full_name\": \"other/repo\"",
    );
    assert_ne!(elsewhere, EVENT);
    let signature = signed("hook-secret", elsewhere.as_bytes());
    let headers = [
        ("X-Hub-Signature-256", signature.as_str()),
        ("X-GitHub-Event", "release"),
    ];
    for _ in 0..2 {
        let answered = world.answer(&hooked("/hooks/gate", &headers, elsewhere.as_bytes()));
        assert_eq!(answered.reply.status, 400);
    }
}

fn inboxed<'a>(
    path: &'a str,
    query: Option<&'a str>,
    headers: &'a [(&'a str, &'a str)],
) -> Inbound<'a> {
    Inbound {
        method: "POST",
        path,
        query,
        headers,
        body: FOLLOW,
    }
}

fn keyed() -> Script {
    let owned = document(ANA);
    Script::answering(&[(ANA, 200, owned.as_str())])
        .headed(&[("content-type", "application/activity+json")])
}

#[test]
fn an_activity_signed_by_its_actor_is_answered_and_left_to_take_once() {
    let world = World::new(keyed());
    let signed = activity(FOLLOW, ANA_KEY, None);
    let headers = borrowed(&signed);
    let answered = world.answer(&inboxed("/inbox", None, &headers));
    assert_eq!(
        said(&answered),
        (202, TEXT, "herald takes the activity".to_owned())
    );
    let Some(Job::Follow { verified, body }) = answered.job else {
        panic!("the activity is left to take");
    };
    assert_eq!(verified.signer.as_deref(), Some(ANA));
    assert_eq!(body, FOLLOW);
    assert_eq!(
        world
            .answer(&inboxed("/inbox", None, &headers))
            .reply
            .status,
        409
    );
    assert!(matches!(
        world.work(Job::Follow { verified, body }),
        Worked::Followed(Ok(Followed::Ignored(_)))
    ));
}

#[test]
fn an_activity_herald_cannot_take_is_answered_by_why() {
    let world = World::new(Script::default());
    let signed = activity(FOLLOW, ANA_KEY, None);
    let mut elsewhere: Vec<(&str, &str)> = borrowed(&signed);
    elsewhere[0] = ("Host", "other.example");
    assert_eq!(
        world
            .answer(&inboxed("/inbox", None, &elsewhere))
            .reply
            .status,
        404
    );
    assert_eq!(
        world
            .answer(&inboxed("/users/php-mutation-gate/inbox", None, &elsewhere))
            .reply
            .status,
        404
    );
    let headers = borrowed(&signed);
    assert_eq!(
        world
            .answer(&inboxed("/inbox", Some("page=1"), &headers))
            .reply
            .status,
        401
    );
    let unadmitted = World::failing(Fault::Admits);
    let unadmitted = World {
        script: keyed(),
        connected: {
            let prepared =
                pollster::block_on(prepare(&unadmitted.config, &secrets(), &FakeSigner)).unwrap();
            prepared.connect(|_| Ok::<_, ()>(keyed())).unwrap()
        },
        ..unadmitted
    };
    assert_eq!(
        unadmitted
            .answer(&inboxed("/inbox", None, &headers))
            .reply
            .status,
        503
    );
}

#[test]
fn work_that_stops_or_fails_releases_its_delivery() {
    let world = World::failing(Fault::Claims);
    let event = herald_sources_forge::Forge::GitHub
        .event(EVENT.as_bytes())
        .unwrap()
        .unwrap();
    let verified = herald_flows::Verified {
        delivery: Some("d1".to_owned()),
        signer: None,
    };
    assert_eq!(world.admits(Scheme::GitHub, "d1"), Admitted::Fresh);
    let Worked::Announced(announced) = world.work(Job::Announce {
        source: herald_core::SourceName::new("gate"),
        event,
        scheme: Scheme::GitHub,
        verified,
    }) else {
        panic!("the release is announced");
    };
    assert!(matches!(announced[0].outcome, Outcome::Stopped { .. }));
    assert_eq!(world.admits(Scheme::GitHub, "d1"), Admitted::Fresh);
    let follow = serde_json::json!({ "id": "https://social.example/f/1", "type": "Follow", "actor": ANA, "object": ACTOR })
        .to_string()
        .into_bytes();
    let verified = herald_flows::Verified {
        delivery: Some("d2".to_owned()),
        signer: Some(ANA.to_owned()),
    };
    assert_eq!(world.admits(Scheme::ActivityPub, "d2"), Admitted::Fresh);
    let Worked::Followed(taken) = world.work(Job::Follow {
        verified,
        body: follow,
    }) else {
        panic!("the activity is taken");
    };
    assert!(matches!(taken, Err(FollowError::Unfetched(_))));
    assert_eq!(world.admits(Scheme::ActivityPub, "d2"), Admitted::Fresh);
}

#[test]
fn work_that_will_not_be_done_releases_its_delivery() {
    let world = World::new(Script::default());
    let event = herald_sources_forge::Forge::GitHub
        .event(EVENT.as_bytes())
        .unwrap()
        .unwrap();
    let jobs = [
        (
            Scheme::GitHub,
            Job::Announce {
                source: herald_core::SourceName::new("gate"),
                event,
                scheme: Scheme::GitHub,
                verified: herald_flows::Verified {
                    delivery: Some("d3".to_owned()),
                    signer: None,
                },
            },
        ),
        (
            Scheme::ActivityPub,
            Job::Follow {
                verified: herald_flows::Verified {
                    delivery: Some("d3".to_owned()),
                    signer: Some(ANA.to_owned()),
                },
                body: FOLLOW.to_vec(),
            },
        ),
    ];
    for (scheme, job) in jobs {
        assert_eq!(world.admits(scheme, "d3"), Admitted::Fresh);
        world.serving(|serving| pollster::block_on(unworked(serving, &job)));
        assert_eq!(world.admits(scheme, "d3"), Admitted::Fresh);
    }
}

#[test]
fn a_webhook_secret_that_is_not_set_stops_the_server_before_it_serves() {
    let config = config();
    let unset = FakeSecrets::default().with("HERALD_ACTIVITYPUB_KEY", "the-key");
    assert!(pollster::block_on(webhooks(&config, &unset)).is_err());
}
