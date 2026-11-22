//! An event pushed to a peer's herald source: as an envelope proved by the
//! instance's newest key, in a request that key signs, once for each
//! revision, private events only to a peer that takes them, and withdrawn
//! by a tombstone once it is not to be seen there.

use herald_config::{Config, Uses, load};
use herald_core::{Audited, Entry, Event, Identity, Key, RouteId, SourceName, Step, Visibility};
use herald_dest_herald::PushOptions;
use herald_flows::{Connected, Outcome, Ports, Stop, announce, prepare, trial};
use herald_peers::digest::content_digest;
use herald_peers::keys::method;
use herald_peers::multibase::ed25519_public;
use herald_ports::{Algorithm, CryptoError, Ledger, Secret, Signer};
use herald_testing::{FakeClock, FakeSecrets, FakeSigner, fake_ed25519};
use jiff::SignedDuration;
use serde_json::Value;

use crate::harness::{Failing, Fault, NOON, Script, event, healthy, outcome};
use crate::pulls::{DOMAIN, OLD};

/// Where the peer's inbox is.
pub(crate) const PUSHED_TO: &str = "POST https://herald.example/herald/v1/inbox/inbox";
/// A request for the run's ID token meant for the peer.
const ASKED: &str =
    "GET «secret:ACTIONS_ID_TOKEN_REQUEST_URL»&audience=https%3A%2F%2Fherald.example";

/// Why an event that is not public is pushed to no peer that takes no
/// private events.
const UNSHARED: &str =
    "the event is not public, and the peer takes no private events, so it is pushed none";

fn config(peer: &str) -> Config {
    configured(peer, "")
}

/// The config of a push to `receiver`, the peer's table carrying `peer`
/// and the push `push`.
fn configured(peer: &str, push: &str) -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[federation]
domain = "{DOMAIN}"
keys = [{{ secret = "PUSH_KEY" }}]
[markers]
key = {{ secret = "HERALD_MARKER" }}
[doctor]
private_to_public = "warn"
[peers.receiver]
url = "https://herald.example"
keys = ["{}"]
{peer}
[destinations.mirror]
kind = "herald-push"
peer = "receiver"
inbox = "inbox"
waits = {{ outage = "40s" }}
{push}
[sources.gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"
[[routes]]
id = "pushed"
sources = ["gate"]
destination = "mirror"
edits = "release"
withdraw = "delete"
"#,
        ed25519_public(&[5; 32])
    );
    load(
        "herald.toml",
        &text,
        &|_| Err("no file".to_owned()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

/// A signer whose signatures are as long as an Ed25519 signature, as a
/// peer reading a proof demands.
struct Sized;

impl Signer for Sized {
    async fn sign(
        &self,
        _: Algorithm,
        key: &Secret,
        message: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        Ok(fake_ed25519(key.expose().as_bytes(), message))
    }

    async fn public(&self, algorithm: Algorithm, key: &Secret) -> Result<Vec<u8>, CryptoError> {
        FakeSigner.public(algorithm, key).await
    }
}

/// An instance pushing each event of `gate` to its peer `receiver`.
pub(crate) struct Sender {
    config: Config,
    connected: Connected<Script>,
    pub(crate) script: Script,
    pub(crate) store: Failing,
}

impl Sender {
    /// One whose peer's inbox answers `answers` in turn, the peer taking
    /// private events where `private`.
    pub(crate) fn new(private: bool, answers: &[(&str, u16, &str)]) -> Self {
        Self::of(config(if private { "private = true" } else { "" }), answers)
    }

    /// One presenting the run's ID token `token`, its push carrying
    /// `push`, whose peer's inbox answers `answers` in turn.
    fn presenting(token: &str, push: &str, answers: &[(&str, u16, &str)]) -> Self {
        Self::holding(
            configured(
                "",
                &format!("credential = \"id-token\"\nid_token = \"variables\"\n{push}"),
            ),
            token,
            answers,
        )
    }

    fn of(config: Config, answers: &[(&str, u16, &str)]) -> Self {
        Self::holding(config, "", answers)
    }

    /// [`Sender::of`], the run's ID token `token`.
    fn holding(config: Config, token: &str, answers: &[(&str, u16, &str)]) -> Self {
        Self::reading(config, (token, token), answers)
    }

    /// [`Sender::of`], the run's variables `HERALD_ID_TOKEN` and
    /// `HERALD_ID_TOKEN_2` holding `tokens`.
    fn reading(config: Config, tokens: (&str, &str), answers: &[(&str, u16, &str)]) -> Self {
        let secrets = FakeSecrets::default()
            .with("PUSH_KEY", str::from_utf8(&OLD).unwrap())
            .with("HERALD_MARKER", "marker-key")
            .with("HERALD_ID_TOKEN", tokens.0)
            .with("HERALD_ID_TOKEN_2", tokens.1)
            .with(
                PushOptions::ACTIONS_URL,
                "https://actions.example/idtoken?api-version=2.0",
            )
            .with(PushOptions::ACTIONS_TOKEN, "asking");
        let prepared =
            pollster::block_on(prepare(&config, &secrets, &FakeSigner, Uses::Announces)).unwrap();
        let script = Script::answering(answers);
        let handed = script.clone();
        let connected = prepared
            .connect(move |_| Ok::<_, ()>(handed.clone()))
            .unwrap();
        Self {
            config,
            connected,
            script,
            store: healthy(),
        }
    }

    pub(crate) fn announced(&self, event: &Event) -> Outcome {
        self.signed(&FakeSigner, event)
    }

    /// What each route did with `event`.
    fn announced_all(&self, event: &Event) -> Vec<herald_flows::Announced> {
        let ports = Ports {
            store: &self.store,
            clock: &FakeClock::at(NOON),
            signer: &FakeSigner,
            budget: herald_flows::Budget::UNBOUNDED,
        };
        pollster::block_on(announce(
            &self.config,
            &self.connected,
            &ports,
            &SourceName::new("gate"),
            event,
            "r1",
        ))
    }

    /// [`Sender::announced`], each signature as long as an Ed25519
    /// signature, so a peer's inbox reads its proof.
    pub(crate) fn announced_to_a_peer(&self, event: &Event) -> Outcome {
        self.signed(&Sized, event)
    }

    fn signed<G: Signer>(&self, signer: &G, event: &Event) -> Outcome {
        let ports = Ports {
            store: &self.store,
            clock: &FakeClock::at(NOON),
            signer,
            budget: herald_flows::Budget::UNBOUNDED,
        };
        let announced = pollster::block_on(announce(
            &self.config,
            &self.connected,
            &ports,
            &SourceName::new("gate"),
            event,
            "r1",
        ));
        outcome(&announced).clone()
    }

    fn tried(&self) -> Vec<herald_flows::Announced> {
        let ports = Ports {
            store: &self.store,
            clock: &FakeClock::at(NOON),
            signer: &FakeSigner,
            budget: herald_flows::Budget::UNBOUNDED,
        };
        pollster::block_on(trial(&self.config, &self.connected, &ports, "doctor"))
    }

    fn entry(&self, version: &str) -> Option<Entry> {
        let key = Key::derive(
            &RouteId::new("pushed"),
            &Identity::new(format!("github.com/1/{version}")),
        );
        pollster::block_on(self.store.inner.entry(&key))
            .unwrap()
            .map(|kept| kept.value)
    }

    /// The envelope of each request, in order.
    pub(crate) fn pushed(&self) -> Vec<Value> {
        self.script
            .bodies()
            .iter()
            .map(|body| serde_json::from_slice(body).unwrap())
            .collect()
    }
}

fn sent() -> Outcome {
    Outcome::Sent {
        steps: vec![Step::Post],
    }
}

fn stop(outcome: &Outcome) -> &Stop {
    match outcome {
        Outcome::Stopped { reason, .. } => reason,
        other => panic!("{other:?}"),
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> &'a str {
    headers
        .iter()
        .find(|(held, _)| held == name)
        .map(|(_, value)| value.as_str())
        .unwrap_or_default()
}

#[test]
fn a_public_event_is_pushed_once_proved_and_signed_by_the_instance_s_newest_key() {
    let sender = Sender::new(false, &[(PUSHED_TO, 202, "taken")]);
    assert_eq!(sender.announced(&event("v1")), sent());
    let [envelope] = sender.pushed().try_into().unwrap();
    assert_eq!(envelope["identity"], "github.com/1/v1");
    assert_eq!(envelope["visibility"], "public");
    assert_eq!(
        envelope["proof"]["verificationMethod"],
        method(DOMAIN, &OLD)
    );
    let [headers] = sender.script.asked().try_into().unwrap();
    assert_eq!(header(&headers, "content-type"), "application/json");
    assert_eq!(
        header(&headers, "content-digest"),
        content_digest(&sender.script.bodies()[0])
    );
    let input = header(&headers, "signature-input");
    for part in [
        "\"@method\" \"@target-uri\" \"content-type\" \"content-digest\"",
        &format!("keyid=\"{}\"", method(DOMAIN, &OLD)),
        "nonce=\"",
        &format!("expires={}", NOON.as_second() + 300),
    ] {
        assert!(input.contains(part), "{input} lacks {part}");
    }
    assert_eq!(
        sender.entry("v1").unwrap().message(&Step::Post),
        Some("github.com/1/v1")
    );
    assert_eq!(sender.announced(&event("v1")), Outcome::AlreadyAnnounced);
    assert_eq!(sender.script.seen().len(), 1);
}

/// A run's ID token meant for `audience`.
fn meant_for(audience: &str) -> String {
    let mut claims = crate::serving::oidc::run("42");
    claims["aud"] = serde_json::json!(audience);
    crate::serving::oidc::id_token(&crate::serving::oidc::header("k1"), &claims)
}

#[test]
fn a_run_presents_its_id_token_and_pushes_the_envelope_for_the_peer_to_prove() {
    let token = meant_for("https://herald.example");
    let sender = Sender::presenting(&token, "", &[(PUSHED_TO, 202, "taken")]);
    assert_eq!(sender.announced(&event("v1")), sent());
    let [envelope] = sender.pushed().try_into().unwrap();
    assert_eq!(envelope["identity"], "github.com/1/v1");
    assert_eq!(envelope.get("proof"), None);
    let [headers] = sender.script.asked().try_into().unwrap();
    assert_eq!(header(&headers, "Authorization"), format!("Bearer {token}"));
    assert_eq!(header(&headers, "content-type"), "application/json");
    assert_eq!(header(&headers, "signature-input"), "");
    assert_eq!(sender.script.most(), [Audited::NAMED_MOST]);
    let refused = Sender::presenting(
        &token,
        "",
        &[(PUSHED_TO, 401, "the ID token is not meant for this herald")],
    );
    let Stop::Refused(why) = stop(&refused.announced(&event("v1"))).clone() else {
        panic!("refused");
    };
    assert!(
        why.contains("the ID token is not meant for this herald"),
        "{why}"
    );
    assert_eq!(
        refused
            .entry("v1")
            .and_then(|entry| entry.message(&Step::Post).map(str::to_owned)),
        None
    );
}

#[test]
fn a_run_s_id_token_meant_for_another_herald_is_never_sent() {
    let elsewhere = meant_for("https://other.example");
    for (token, push) in [
        (elsewhere.as_str(), ""),
        ("not an ID token", ""),
        (
            &meant_for("https://herald.example") as &str,
            "audience = \"https://herald.example/ci\"",
        ),
    ] {
        let sender = Sender::presenting(token, push, &[]);
        let Stop::Refused(why) = stop(&sender.announced(&event("v1"))).clone() else {
            panic!("refused");
        };
        assert!(why.contains("the run's ID token is not meant for"), "{why}");
        assert!(!why.contains(token), "{why}");
        assert_eq!(sender.script.seen(), Vec::<String>::new());
    }
    let named = Sender::presenting(
        &meant_for("https://herald.example/ci"),
        "audience = \"https://herald.example/ci\"",
        &[(PUSHED_TO, 202, "taken")],
    );
    assert_eq!(named.announced(&event("v1")), sent());
}

#[test]
fn a_run_on_github_actions_asks_for_a_fresh_id_token_for_each_push() {
    let first = meant_for("https://herald.example");
    let mut later = crate::serving::oidc::run("42");
    later["jti"] = serde_json::json!("second");
    let second = crate::serving::oidc::id_token(&crate::serving::oidc::header("k1"), &later);
    let answer = |token: &str| serde_json::json!({ "count": 1, "value": token }).to_string();
    let (one, two) = (answer(&first), answer(&second));
    let sender = Sender::holding(
        configured("", "credential = \"id-token\""),
        "",
        &[
            (ASKED, 200, &one),
            (PUSHED_TO, 202, "taken"),
            (ASKED, 200, &two),
            (PUSHED_TO, 202, "taken"),
        ],
    );
    assert_eq!(sender.announced(&event("v1")), sent());
    assert_eq!(sender.announced(&event("v2")), sent());
    assert_eq!(sender.script.seen(), [ASKED, PUSHED_TO, ASKED, PUSHED_TO]);
    let asked = sender.script.asked();
    assert_eq!(header(&asked[0], "Authorization"), "Bearer asking");
    assert_eq!(
        header(&asked[1], "Authorization"),
        format!("Bearer {first}")
    );
    assert_eq!(
        header(&asked[3], "Authorization"),
        format!("Bearer {second}")
    );
    for (status, body, why) in [
        (
            403,
            String::new(),
            "the CI answered 403 when asked for the run's ID token",
        ),
        (
            200,
            "{}".to_owned(),
            "the CI answered no ID token when asked for one",
        ),
        (
            200,
            answer(&meant_for("https://other.example")),
            "the run's ID token is not meant for https://herald.example",
        ),
    ] {
        let refused = Sender::holding(
            configured("", "credential = \"id-token\""),
            "",
            &[(ASKED, status, &body)],
        );
        let Stop::Refused(said) = stop(&refused.announced(&event("v1"))).clone() else {
            panic!("refused");
        };
        assert!(said.starts_with(why), "{said}");
        assert_eq!(refused.script.seen(), [ASKED]);
    }
    let unreached = Sender::holding(configured("", "credential = \"id-token\""), "", &[]);
    assert!(matches!(
        stop(&unreached.announced(&event("v1"))),
        Stop::Unreachable(said) if said.starts_with("the run's ID token: ")
    ));
}

#[test]
fn a_run_presents_each_id_token_it_read_once_and_leaves_the_rest_to_a_later_run() {
    let token = meant_for("https://herald.example");
    let sender = Sender::presenting(
        &token,
        "id_tokens = [{ secret = \"HERALD_ID_TOKEN\" }, { secret = \"HERALD_ID_TOKEN_2\" }]",
        &[(PUSHED_TO, 202, "taken"), (PUSHED_TO, 202, "taken")],
    );
    assert_eq!(sender.announced(&event("v1")), sent());
    assert_eq!(sender.announced(&event("v2")), sent());
    assert!(matches!(
        stop(&sender.announced(&event("v3"))),
        Stop::Limit(said) if said.starts_with("every ID token the run read was presented once")
    ));
    assert_eq!(sender.script.seen(), [PUSHED_TO, PUSHED_TO]);
    assert_eq!(
        sender
            .entry("v3")
            .and_then(|entry| entry.message(&Step::Post).map(str::to_owned)),
        None
    );
}

#[test]
fn a_peer_asking_to_wait_or_refusing_stops_the_route_and_lands_nothing() {
    let waited = Sender::new(false, &[(PUSHED_TO, 503, "")]);
    assert_eq!(
        stop(&waited.announced(&event("v1"))),
        &Stop::RetryAfter(SignedDuration::from_secs(40))
    );
    let refused = Sender::new(false, &[(PUSHED_TO, 403, "the source grants no push")]);
    let Stop::Refused(why) = stop(&refused.announced(&event("v1"))).clone() else {
        panic!("refused");
    };
    assert!(
        why.starts_with("the peer's inbox refused it with 403: the source grants no push"),
        "{why}"
    );
    assert_eq!(refused.entry("v1"), None);
    let said = "x".repeat(Audited::NAMED_MOST);
    let rambling = Sender::new(false, &[(PUSHED_TO, 400, &said)]);
    let Stop::Refused(why) = stop(&rambling.announced(&event("v1"))).clone() else {
        panic!("refused");
    };
    assert!(why.contains(&said), "{why}");
    let longer = format!("{said}x");
    let oversized = Sender::new(false, &[(PUSHED_TO, 202, &longer)]);
    assert_eq!(
        stop(&oversized.announced(&event("v1"))),
        &Stop::Unreachable(format!(
            "the response is larger than {} bytes",
            Audited::NAMED_MOST
        ))
    );
    assert_eq!(oversized.script.most(), [Audited::NAMED_MOST]);
    assert_eq!(oversized.entry("v1"), None);
    let unreachable = Sender::new(false, &[]);
    assert!(matches!(
        stop(&unreachable.announced(&event("v1"))),
        Stop::Unreachable(_)
    ));
    let mut unread = Sender::new(false, &[]);
    unread.store.fault = Fault::Reads;
    assert!(matches!(
        stop(&unread.announced(&event("v1"))),
        Stop::Store(_)
    ));
    assert_eq!(unread.script.seen(), Vec::<String>::new());
}

#[test]
fn an_event_too_large_for_any_envelope_is_refused_unpushed() {
    let sender = Sender::new(false, &[]);
    let mut written = serde_json::to_value(event("v1")).unwrap();
    written["project"] = "p".repeat(2000).into();
    let long: Event = serde_json::from_value(written).unwrap();
    let Stop::Refused(why) = stop(&sender.announced(&long)).clone() else {
        panic!("refused");
    };
    assert!(why.contains("project"), "{why}");
    assert_eq!(sender.script.seen(), Vec::<String>::new());
}

#[test]
fn an_event_that_is_not_public_is_pushed_only_to_a_peer_taking_private_events() {
    let closed = Sender::new(false, &[]);
    for visibility in [Visibility::Private, Visibility::Internal] {
        assert_eq!(
            closed.announced(&event("v1").seen_as(visibility)),
            Outcome::Skipped(UNSHARED.to_owned()),
            "{visibility:?}"
        );
    }
    assert_eq!(closed.script.seen(), Vec::<String>::new());
    let open = Sender::new(true, &[(PUSHED_TO, 202, "taken")]);
    assert_eq!(
        open.announced(&event("v1").seen_as(Visibility::Private)),
        sent()
    );
    assert_eq!(open.pushed()[0]["visibility"], "private");
}

#[test]
fn an_event_no_longer_public_is_withdrawn_from_the_peer_by_a_tombstone() {
    let sender = Sender::new(
        false,
        &[
            (PUSHED_TO, 202, "taken"),
            (PUSHED_TO, 202, "taken"),
            (PUSHED_TO, 202, "taken"),
        ],
    );
    assert_eq!(sender.announced(&event("v1")), sent());
    let hidden = event("v1").seen_as(Visibility::Private);
    assert_eq!(
        sender.announced(&hidden),
        Outcome::Skipped(UNSHARED.to_owned())
    );
    let tombstone = &sender.pushed()[1];
    assert_eq!(tombstone["type"], "herald:Tombstone");
    assert_eq!(tombstone["identity"], "github.com/1/v1");
    assert_eq!(sender.entry("v1").unwrap().message(&Step::Post), None);
    assert_eq!(
        sender.announced(&hidden),
        Outcome::Skipped(UNSHARED.to_owned())
    );
    assert_eq!(sender.script.seen().len(), 2);
    assert_eq!(sender.announced(&event("v1")), sent());
    let refusing = Sender::new(false, &[(PUSHED_TO, 202, "taken"), (PUSHED_TO, 403, "no")]);
    refusing.announced(&event("v1"));
    assert!(matches!(
        stop(&refusing.announced(&hidden)),
        Stop::Refused(_)
    ));
}

#[test]
fn a_withdrawn_release_is_withdrawn_from_the_peer_by_a_tombstone_once() {
    let sender = Sender::new(
        false,
        &[(PUSHED_TO, 202, "taken"), (PUSHED_TO, 202, "taken")],
    );
    assert_eq!(sender.announced(&event("v1")), sent());
    assert_eq!(
        sender.announced(&event("v1").marked_withdrawn()),
        Outcome::Sent {
            steps: vec![Step::Delete(0)],
        }
    );
    assert_eq!(sender.pushed()[1]["type"], "herald:Tombstone");
    assert_eq!(
        sender.announced(&event("v1").marked_withdrawn()),
        Outcome::Skipped(crate::withdraw::UNANNOUNCED.to_owned())
    );
    assert_eq!(sender.script.seen().len(), 2);
    let refusing = Sender::new(false, &[(PUSHED_TO, 202, "taken"), (PUSHED_TO, 403, "no")]);
    refusing.announced(&event("v1"));
    assert!(matches!(
        refusing.announced(&event("v1").marked_withdrawn()),
        Outcome::Stopped { .. }
    ));
}

#[test]
fn a_test_message_is_pushed_to_no_peer() {
    let sender = Sender::new(true, &[(PUSHED_TO, 202, "taken")]);
    assert_eq!(sender.tried(), Vec::new());
    assert_eq!(sender.script.seen(), Vec::<String>::new());
}

#[test]
fn doctor_counts_a_route_to_a_push_as_federation_s() {
    let sender = Sender::new(true, &[]);
    let secrets = FakeSecrets::default().with("PUSH_KEY", str::from_utf8(&OLD).unwrap());
    let layer = crate::doctoring::actions();
    let shown = pollster::block_on(herald_flows::federation(
        &sender.config,
        &layer,
        (&secrets, &FakeSigner),
        &[],
    ))
    .unwrap();
    assert_eq!(shown.routes, [0]);
    assert!(shown.owns("routes[0].withdraw"));
    assert_eq!(
        shown.keys,
        [herald_flows::OwnKey {
            secret: "PUSH_KEY".to_owned(),
            shown: Some(herald_flows::Shown::of(&OLD)),
        }]
    );
}

#[test]
fn a_run_s_destinations_naming_one_variable_present_its_token_once() {
    let token = meant_for("https://herald.example");
    let again = "[destinations.again]\nkind = \"herald-push\"\npeer = \"receiver\"\ninbox = \"inbox\"\ncredential = \"id-token\"\nid_token = \"variables\"\n\
                 [[routes]]\nid = \"again\"\nsources = [\"gate\"]\ndestination = \"again\"\n";
    let sender = Sender::presenting(&token, again, &[(PUSHED_TO, 202, "taken")]);
    let announced = sender.announced_all(&event("v1"));
    assert_eq!(sender.script.seen(), [PUSHED_TO]);
    let limited = announced
        .iter()
        .filter(|announced| {
            matches!(
                &announced.outcome,
                Outcome::Stopped {
                    reason: Stop::Limit(_),
                    ..
                }
            )
        })
        .count();
    assert_eq!(limited, 1, "{announced:?}");
}

#[test]
fn a_token_whose_push_never_left_is_presented_again_and_one_that_may_have_is_not() {
    let token = meant_for("https://herald.example");
    let unconnected = Sender::presenting(
        &token,
        "",
        &[
            (PUSHED_TO, crate::harness::UNCONNECTED, "connection refused"),
            (PUSHED_TO, 202, "taken"),
        ],
    );
    assert!(matches!(
        stop(&unconnected.announced(&event("v1"))),
        Stop::Unreachable(said) if said.ends_with("connection refused")
    ));
    assert_eq!(unconnected.announced(&event("v2")), sent());
    let lost = Sender::presenting(&token, "", &[]);
    assert!(matches!(
        stop(&lost.announced(&event("v1"))),
        Stop::Unreachable(_)
    ));
    assert!(matches!(
        stop(&lost.announced(&event("v2"))),
        Stop::Limit(_)
    ));
    assert_eq!(lost.script.seen(), [PUSHED_TO]);
}

#[test]
fn a_token_read_for_another_herald_is_passed_over_and_left_unpresented() {
    let (other, ours) = (
        meant_for("https://other.example"),
        meant_for("https://herald.example"),
    );
    let both = "credential = \"id-token\"\nid_token = \"variables\"\n\
                id_tokens = [{ secret = \"HERALD_ID_TOKEN\" }, { secret = \"HERALD_ID_TOKEN_2\" }]";
    let sender = Sender::reading(
        configured("", both),
        (&other, &ours),
        &[(PUSHED_TO, 202, "taken")],
    );
    assert_eq!(sender.announced(&event("v1")), sent());
    let [headers] = sender.script.asked().try_into().unwrap();
    assert_eq!(header(&headers, "Authorization"), format!("Bearer {ours}"));
    let Stop::Refused(why) = stop(&sender.announced(&event("v2"))).clone() else {
        panic!("refused");
    };
    assert!(why.contains("the run's ID token is not meant for"), "{why}");
    assert_eq!(sender.script.seen(), [PUSHED_TO]);
}

#[test]
fn a_token_that_cannot_be_read_is_never_presented_nor_spent() {
    let both = "credential = \"id-token\"\nid_token = \"variables\"\n\
                id_tokens = [{ secret = \"HERALD_ID_TOKEN\" }, { secret = \"HERALD_ID_TOKEN_2\" }]";
    let sender = Sender::reading(
        configured("", both),
        ("unreadable", "not.a.token"),
        &[(PUSHED_TO, 202, "taken")],
    );
    for version in ["v1", "v2"] {
        let Stop::Refused(why) = stop(&sender.announced(&event(version))).clone() else {
            panic!("refused, not spent");
        };
        assert!(why.contains("the run's ID token is not meant for"), "{why}");
    }
    assert_eq!(sender.script.seen(), Vec::<String>::new());
}
