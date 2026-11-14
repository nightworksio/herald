use herald_config::{Config, Uses, load};
use herald_core::{Key, RouteId, Step};
use herald_flows::{
    Answered, Connected, Did, FollowError, Followed, Inbound, Job, Outcome, Ports, Serving, Stop,
    answer, busy, prepare, unplaced, unworked, work,
};
use herald_ports::{Admitted, Algorithm, CryptoError, Deliveries, Ledger, Secret, Signer};
use herald_records::MemoryKeyring;
use herald_testing::{FakeClock, FakeSecrets, FakeSigner, FakeVerifier};

use crate::harness::{Failing, Fault, Script, broken, healthy, keys_in};
use crate::inbound::{ANA, ANA_KEY, FOLLOW, activity, at, borrowed, document};

pub(crate) const EVENT: &str =
    include_str!("../../../herald-sources-forge/fixtures/github/release-event.reference.json");
pub(crate) const ACTOR: &str = "https://herald.example/users/php-mutation-gate";

const APP_TOKEN: &str = "app-token";

fn config() -> Config {
    load(
        "herald.toml",
        &config_text(),
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

pub(crate) fn config_text() -> String {
    r#"version = 1
[store]
kind = "sqlite"
[destinations.fedi]
kind = "activitypub"
domain = "herald.example"
key = { secret = "HERALD_ACTIVITYPUB_KEY" }
outbox_page = 2
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
[[routes]]
id = "deployed"
sources = ["ci"]
destination = "fedi"
template = "t"
actor = "deploys"
[[routes]]
id = "elsewhere"
sources = ["ci"]
destination = "fedi"
template = "t"
actor = "deploys"
[apps.ci]
tokens = ["sha256:TOKEN_HASH"]
projects = ["herald", "night works"]
routes = ["deployed"]
[templates.t]
title = "{{ event.title }}"
body = "{{ event.body }}"
"#
    .replace("TOKEN_HASH", &herald_core::digest(APP_TOKEN))
}

fn secrets() -> FakeSecrets {
    FakeSecrets::default()
        .with("HERALD_ACTIVITYPUB_KEY", "the-key")
        .with("HERALD_WEBHOOK", "hook-secret")
}

pub(crate) struct World {
    config: Config,
    connected: Connected<Script>,
    pub(crate) store: Failing,
    keyring: MemoryKeyring,
    script: Script,
}

type Served<'a> =
    Serving<'a, Failing, FakeClock, FakeSigner, FakeVerifier, Script, Script, MemoryKeyring>;

impl World {
    pub(crate) fn new(script: Script) -> Self {
        Self::with(config(), script)
    }

    pub(crate) fn with(config: Config, script: Script) -> Self {
        let prepared =
            pollster::block_on(prepare(&config, &secrets(), &FakeSigner, Uses::Serves)).unwrap();
        let connected = prepared.connect(|_| Ok::<_, ()>(script.clone())).unwrap();
        Self {
            config,
            connected,
            store: healthy(),
            keyring: MemoryKeyring::new(8),
            script,
        }
    }

    pub(crate) fn failing(fault: Fault) -> Self {
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
        let keys = keys_in(&self.keyring);
        act(&Serving {
            config: &self.config,
            connected: &self.connected,
            ports: &ports,
            verifier: &FakeVerifier,
            public: &self.script,
            keys: &keys,
            webhooks: self.connected.webhooks(),
            admission: herald_flows::Admission::Once,
        })
    }

    pub(crate) fn answer(&self, inbound: &Inbound<'_>) -> Answered {
        self.serving(|serving| pollster::block_on(answer(serving, inbound)))
    }

    fn work(&self, job: Job) -> Did {
        let worked = self.serving(|serving| pollster::block_on(work(serving, job, "server")));
        assert_eq!(worked.unreleased, None);
        worked.did
    }

    fn admits(&self, namespace: &'static str, delivery: &str) -> Admitted {
        pollster::block_on(self.store.inner.admit(namespace, delivery, at())).unwrap()
    }
}

pub(crate) fn get(path: &str) -> Inbound<'_> {
    Inbound {
        method: "GET",
        path,
        query: None,
        headers: &[("Host", "herald.example")],
        body: &[],
    }
}

pub(crate) fn said(answered: &Answered) -> (u16, &'static str, String) {
    (
        answered.reply.status,
        answered.reply.kind,
        String::from_utf8(answered.reply.body.clone()).unwrap(),
    )
}

const TEXT: &str = "text/plain; charset=utf-8";

/// A webhook's path, its headers and the status it is answered with.
type Case<'a> = (&'a str, &'a [(&'a str, &'a str)], u16);
pub(crate) const JSON: &str = "application/activity+json";

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
        "/users/php-mutation-gate/following",
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
fn every_path_is_served_under_the_base_and_none_outside_it() {
    let mut world = World::new(Script::default());
    world.config.server.base = "/tools/herald".to_owned();
    assert_eq!(
        said(&world.answer(&get("/tools/herald/healthz"))),
        (200, "text/plain; charset=utf-8", "herald is up".to_owned())
    );
    for outside in ["/healthz", "/tools/heraldry/healthz", "/tools"] {
        assert_eq!(said(&world.answer(&get(outside))).0, 404, "{outside}");
    }
    assert_eq!(said(&world.answer(&get("/tools/herald"))).0, 404);
}

#[test]
fn work_with_no_place_is_refused_and_asked_to_wait() {
    let refused = busy(jiff::SignedDuration::from_secs(30));
    assert_eq!(
        said(&refused),
        (
            503,
            TEXT,
            "herald cannot answer this request now".to_owned()
        )
    );
    assert_eq!(refused.reply.retry_after, Some(30));
    for (after, asked) in [
        (jiff::SignedDuration::from_millis(1_500), 2),
        (jiff::SignedDuration::from_millis(1), 1),
        (jiff::SignedDuration::ZERO, 1),
        (jiff::SignedDuration::from_hours(24 * 365 * 1_000), u32::MAX),
    ] {
        assert_eq!(busy(after).reply.retry_after, Some(asked), "{after:?}");
    }
    assert_eq!(
        refused.refused.as_deref(),
        Some("the queue has no place for the work it leaves")
    );
    assert!(refused.job.is_none());
}

/// What a route answers when the store cannot say whether its event was
/// delivered before.
fn unadmitted() -> Outcome {
    Outcome::Stopped {
        landed: Vec::new(),
        remaining: Vec::new(),
        reason: Stop::Store("the store could not be reached: broken".to_owned()),
    }
}

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
    let keys = keys_in(&world.keyring);
    let serving = Serving {
        config: &world.config,
        connected: &world.connected,
        ports: &ports,
        verifier: &FakeVerifier,
        public: &world.script,
        keys: &keys,
        webhooks: world.connected.webhooks(),
        admission: herald_flows::Admission::Once,
    };
    let answered = pollster::block_on(answer(&serving, &get("/users/php-mutation-gate")));
    assert_eq!(answered.reply.status, 500);
    assert_eq!(
        String::from_utf8(answered.reply.body).unwrap(),
        "herald cannot answer this request now"
    );
    assert_eq!(
        answered.refused.as_deref(),
        Some(
            "the actor's key cannot be read: the secret HERALD_ACTIVITYPUB_KEY is not a key for this signature; give a PKCS#8 private key as PEM"
        )
    );
    assert_eq!(
        pollster::block_on(Keyless.sign(Algorithm::RsaSha256, &Secret::new("K", "v"), b"")),
        Err(CryptoError::BadKey {
            name: "K".to_owned()
        })
    );
}

pub(crate) fn key() -> Key {
    let event = herald_sources_forge::Forge::GitHub
        .event(EVENT.as_bytes())
        .unwrap()
        .unwrap();
    Key::derive(&RouteId::new("followed"), event.identity())
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
    assert_eq!(
        (source.as_str(), scheme),
        ("gate", herald_sources_forge::Forge::GitHub.scheme())
    );
    let delivery = verified.delivery.clone().unwrap();
    let again = world.answer(&hooked("/hooks/gate", &headers, EVENT.as_bytes()));
    assert_eq!(again.reply.status, 409);
    assert_eq!(
        String::from_utf8(again.reply.body).unwrap(),
        "herald has taken this request already"
    );
    let Some(job) = answered.job else {
        panic!("the release is left to announce");
    };
    assert_eq!(
        world.work(job),
        Did::Announced(vec![herald_flows::Announced {
            route: RouteId::new("followed"),
            outcome: Outcome::Sent {
                steps: vec![Step::Post]
            },
        }])
    );
    assert_eq!(
        world.admits(herald_sources_forge::Forge::GitHub.scheme().name, &delivery),
        Admitted::Seen
    );
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
        assert_eq!(
            String::from_utf8(answered.reply.body).unwrap(),
            "herald does not take this request"
        );
    }
    let unreleased = World::failing(Fault::ReleasesDeliveries).answer(&hooked(
        "/hooks/gate",
        &headers,
        elsewhere.as_bytes(),
    ));
    assert_eq!(unreleased.reply.status, 400);
    assert_eq!(
        unreleased.refused.as_deref(),
        Some(
            "GitHub's release event could not be read: it is not of cli/cli, the repository its source names; its delivery could not be released: the store could not be reached: broken"
        )
    );
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
fn an_actor_is_served_on_its_own_domain_only() {
    let text = config_text().replace(
        "[sources.gate]",
        "[destinations.other]\nkind = \"activitypub\"\ndomain = \"other.example\"\nkey = { secret = \"HERALD_ACTIVITYPUB_KEY\" }\n[sources.gate]",
    ) + "[[routes]]\nid = \"other\"\nsources = [\"gate\"]\ndestination = \"other\"\ntemplate = \"t\"\nactor = \"releases\"\n";
    let world = World::with(
        load(
            "herald.toml",
            &text,
            &|_| Err(String::new()),
            &crate::harness::kinds(),
        )
        .unwrap(),
        keyed(),
    );
    let signed = activity(FOLLOW, ANA_KEY, None);
    let mut other: Vec<(&str, &str)> = borrowed(&signed);
    other[0] = ("Host", "other.example");
    let status = |path| world.answer(&inboxed(path, None, &other)).reply.status;
    assert_eq!(status("/users/php-mutation-gate/inbox"), 404);
    assert_eq!(status("/users/releases/inbox"), 401);
    assert_eq!(status("/inbox"), 401);
    for (host, status) in [("other.example", 200), ("herald.example", 404)] {
        let headers = [("Host", host)];
        let fetched = Inbound {
            headers: &headers,
            ..get("/users/releases")
        };
        assert_eq!(world.answer(&fetched).reply.status, status);
    }
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
    let Did::Followed(followed) = world.work(Job::Follow { verified, body }) else {
        panic!("the follow is taken");
    };
    assert_eq!(
        followed,
        Ok(Followed::Ignored("it is a follow with no id".to_owned()))
    );
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
            let prepared = pollster::block_on(prepare(
                &unadmitted.config,
                &secrets(),
                &FakeSigner,
                Uses::Polls,
            ))
            .unwrap();
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
    assert_eq!(
        world.admits(herald_sources_forge::Forge::GitHub.scheme().name, "d1"),
        Admitted::Fresh
    );
    let Did::Announced(announced) = world.work(Job::Announce {
        source: herald_core::SourceName::new("gate"),
        event,
        scheme: herald_sources_forge::Forge::GitHub.scheme(),
        verified,
    }) else {
        panic!("the release is announced");
    };
    assert_eq!(announced[0].outcome, unadmitted());
    assert_eq!(
        world.admits(herald_sources_forge::Forge::GitHub.scheme().name, "d1"),
        Admitted::Fresh
    );
    let follow = serde_json::json!({ "id": "https://social.example/f/1", "type": "Follow", "actor": ANA, "object": ACTOR })
        .to_string()
        .into_bytes();
    let verified = herald_flows::Verified {
        delivery: Some("d2".to_owned()),
        signer: Some(ANA.to_owned()),
    };
    assert_eq!(
        world.admits(herald_dest_fediverse::ACTOR_SCHEME.name, "d2"),
        Admitted::Fresh
    );
    let Did::Followed(taken) = world.work(Job::Follow {
        verified,
        body: follow,
    }) else {
        panic!("the activity is taken");
    };
    assert_eq!(taken, Err(FollowError::Unfetched("the host could not be reached: nothing scripted for GET https://social.example/users/ana".to_owned())));
    assert_eq!(
        world.admits(herald_dest_fediverse::ACTOR_SCHEME.name, "d2"),
        Admitted::Fresh
    );
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
            herald_sources_forge::Forge::GitHub.scheme(),
            Job::Announce {
                source: herald_core::SourceName::new("gate"),
                event,
                scheme: herald_sources_forge::Forge::GitHub.scheme(),
                verified: herald_flows::Verified {
                    delivery: Some("d3".to_owned()),
                    signer: None,
                },
            },
        ),
        (
            herald_dest_fediverse::ACTOR_SCHEME,
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
        assert_eq!(world.admits(scheme.name, "d3"), Admitted::Fresh);
        assert_eq!(
            world.serving(|serving| pollster::block_on(unworked(serving, &job))),
            Ok(())
        );
        assert_eq!(world.admits(scheme.name, "d3"), Admitted::Fresh);
    }
}

fn followed_by(delivery: &str) -> Job {
    Job::Follow {
        verified: herald_flows::Verified {
            delivery: Some(delivery.to_owned()),
            signer: Some(ANA.to_owned()),
        },
        body: serde_json::json!({ "id": "https://social.example/f/1", "type": "Follow", "actor": ANA, "object": ACTOR })
            .to_string()
            .into_bytes(),
    }
}

fn unreleased() -> String {
    format!("its delivery could not be released: {}", broken())
}

#[test]
fn work_that_stops_and_cannot_release_its_delivery_says_so() {
    let world = World::failing(Fault::ReleasesDeliveries);
    assert_eq!(
        world.admits(herald_dest_fediverse::ACTOR_SCHEME.name, "d4"),
        Admitted::Fresh
    );
    let worked =
        world.serving(|serving| pollster::block_on(work(serving, followed_by("d4"), "server")));
    assert!(
        matches!(worked.did, Did::Followed(Err(FollowError::Unfetched(_)))),
        "{worked:?}"
    );
    assert_eq!(worked.unreleased, Some(unreleased()));
    assert_eq!(
        world.admits(herald_dest_fediverse::ACTOR_SCHEME.name, "d4"),
        Admitted::Seen
    );
    assert_eq!(
        world.serving(|serving| pollster::block_on(unworked(serving, &followed_by("d4")))),
        Err(unreleased())
    );
}

#[test]
fn work_with_no_place_is_refused_once_its_delivery_is_released() {
    let world = World::new(Script::default());
    assert_eq!(
        world.admits(herald_dest_fediverse::ACTOR_SCHEME.name, "d5"),
        Admitted::Fresh
    );
    let refused =
        world.serving(|serving| pollster::block_on(unplaced(serving, &followed_by("d5"))));
    assert_eq!(refused, busy(world.config.server.busy_retry));
    assert_eq!(
        world.admits(herald_dest_fediverse::ACTOR_SCHEME.name, "d5"),
        Admitted::Fresh
    );
}

#[test]
fn work_with_no_place_whose_delivery_cannot_be_released_names_the_release_for_the_log() {
    let world = World::failing(Fault::ReleasesDeliveries);
    assert_eq!(
        world.admits(herald_dest_fediverse::ACTOR_SCHEME.name, "d6"),
        Admitted::Fresh
    );
    let refused =
        world.serving(|serving| pollster::block_on(unplaced(serving, &followed_by("d6"))));
    assert_eq!(refused.reply, busy(world.config.server.busy_retry).reply);
    assert_eq!(
        refused.refused,
        Some(format!(
            "the queue has no place for the work it leaves; {}",
            unreleased()
        ))
    );
    assert!(refused.job.is_none());
}

#[test]
fn a_webhook_secret_that_is_not_set_stops_the_server_before_it_serves() {
    let config = config();
    let unset = FakeSecrets::default().with("HERALD_ACTIVITYPUB_KEY", "the-key");
    let polled = pollster::block_on(prepare(&config, &unset, &FakeSigner, Uses::Polls));
    assert!(polled.is_ok());
    assert_eq!(
        pollster::block_on(prepare(&config, &unset, &FakeSigner, Uses::Serves))
            .err()
            .map(|error| error.to_string()),
        Some(
            "sources.gate.webhook: no secret is named HERALD_WEBHOOK; set it where this platform keeps secrets"
                .to_owned()
        )
    );
}

mod apps;
