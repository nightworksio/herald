//! What Teams posts to a bot at herald's endpoint, against keys and tokens
//! made at run time: each activity taken only on a bearer token
//! signed by a key the Bot Framework's metadata lists and endorses for
//! Teams, of its issuer, meant for the bot, valid now and naming the
//! service URL the activity names, on the bot's own service origin; every
//! other request refused before its body is read, saying nothing of why,
//! and never quoted; a command answered by a reply carrying the bot's token
//! to that service URL alone, its sign-in to the bot's sign-in host alone.

use std::cell::Cell;
use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use herald_config::{Config, Uses, load};
use herald_flows::{
    Answered, Connected, Did, Inbound, Job, Ports, Serving, Webhooks, answer, prepare, work,
};
use herald_ports::{Deliveries as _, Response};
use herald_records::MemoryKeyring;
use herald_testing::{FakeClock, FakeSecrets, FakeSigner, FakeVerifier};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

use crate::harness::{Failing, Script, healthy, keys_in};
use crate::inbound::at;
use crate::serving::said;

const APP: &str = "0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9";
const METADATA: &str = "https://login.example/v1/.well-known/openidconfiguration";
const KEYS: &str = "https://login.example/v1/.well-known/keys";
const ISSUER: &str = "https://api.example";
const SERVICE: &str = "https://smba.example/emea/";
const PATH: &str = "/interactions/teams";
const ACTIVITY_ID: &str = "1481567603816";

/// A key as a key set lists it, its modulus made at run time from `seed`
/// so each pair of a test is its own; the fake verifier takes a signature
/// by it alone.
struct Pair {
    modulus: Vec<u8>,
    exponent: Vec<u8>,
}

fn pair(seed: u8) -> Pair {
    Pair {
        modulus: [0xc1, seed, 3, 4].to_vec(),
        exponent: [1, 0, 1].to_vec(),
    }
}

fn encoded(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// A key set listing `keys`, each by its id with what it is endorsed for.
fn key_set(keys: &[(&str, &Pair, &[&str])]) -> String {
    let listed: Vec<Value> = keys
        .iter()
        .map(|(kid, pair, endorsements)| {
            json!({
                "kty": "RSA",
                "use": "sig",
                "kid": kid,
                "x5t": kid,
                "n": encoded(&pair.modulus),
                "e": encoded(&pair.exponent),
                "endorsements": endorsements,
            })
        })
        .collect();
    json!({ "keys": listed }).to_string()
}

fn metadata(issuer: &str, jwks: &str, algorithms: &[&str]) -> String {
    json!({
        "issuer": issuer,
        "authorization_endpoint": "https://invalid.example",
        "jwks_uri": jwks,
        "id_token_signing_alg_values_supported": algorithms,
        "token_endpoint_auth_methods_supported": ["private_key_jwt"],
    })
    .to_string()
}

fn answered(status: u16, body: &str) -> Response {
    Response {
        status,
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
    }
}

/// The Bot Framework's two documents as `metadata` and `keys` answer them.
fn published(metadata: Response, keys: Response) -> Script {
    Script::keyed(BTreeMap::from([
        (METADATA.to_owned(), metadata),
        (KEYS.to_owned(), keys),
    ]))
}

/// The Bot Framework publishing `teams` endorsed for Teams under `k1`,
/// `speech` under `k2` endorsed for another channel alone.
fn publishing(teams: &Pair, speech: &Pair) -> Script {
    published(
        answered(200, &metadata(ISSUER, KEYS, &["RS256"])),
        answered(
            200,
            &key_set(&[
                ("k1", teams, &["skype", "msteams"]),
                ("k2", speech, &["telephony"]),
            ]),
        ),
    )
}

fn claims(now: Timestamp) -> Value {
    json!({
        "iss": ISSUER,
        "aud": APP,
        "nbf": now.as_second() - 60,
        "exp": now.as_second() + 3600,
        "serviceUrl": SERVICE,
    })
}

/// A token of `claims` under `header`, signed as the fake verifier takes a
/// signature by `pair`.
fn token(header: &Value, claims: &Value, pair: &Pair) -> String {
    let signed = format!(
        "{}.{}",
        encoded(&serde_json::to_vec(header).unwrap()),
        encoded(&serde_json::to_vec(claims).unwrap())
    );
    let key = herald_keys::rsa_public(&pair.modulus, &pair.exponent);
    let signature = [b"RsaSha256".as_slice(), &key, signed.as_bytes()].join(&b'|');
    format!("{signed}.{}", encoded(&signature))
}

fn header(kid: &str) -> Value {
    json!({ "typ": "JWT", "alg": "RS256", "x5t": kid, "kid": kid })
}

fn activity(kind: &str, id: &str, service: &str) -> Vec<u8> {
    json!({
        "type": kind,
        "id": id,
        "timestamp": "2027-01-15T08:00:00.000Z",
        "serviceUrl": service,
        "channelId": "msteams",
        "from": { "id": "29:member", "aadObjectId": "00000000-0000-0000-0000-000000000001" },
        "conversation": { "id": "19:abc@thread.tacv2", "conversationType": "channel" },
        "recipient": { "id": format!("28:{APP}") },
        "text": "<at>herald</at> latest",
    })
    .to_string()
    .into_bytes()
}

fn config() -> Config {
    let text = format!(
        r#"version = 1
[store]
kind = "sqlite"
[destinations.teams]
kind = "teams"
app_id = "{APP}"
app_password = {{ secret = "HERALD_TEAMS_APP_PASSWORD" }}
login = "https://signin.example"
service = "https://smba.example/teams/"
openid = "{METADATA}"
issuer = "{ISSUER}"
[destinations.teams.interactions]
via = "http"
"#
    );
    load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

struct Bot {
    config: Config,
    connected: Connected<Script>,
    bases: Arc<Mutex<Vec<Option<Vec<String>>>>>,
    store: Failing,
    keyring: MemoryKeyring,
    script: Script,
    now: Cell<Timestamp>,
}

impl Bot {
    fn new(script: Script) -> Self {
        let config = config();
        let secrets = FakeSecrets::default().with("HERALD_TEAMS_APP_PASSWORD", "app-secret");
        let prepared =
            pollster::block_on(prepare(&config, &secrets, &FakeSigner, Uses::Serves)).unwrap();
        let bases = Arc::new(Mutex::new(Vec::new()));
        let connected = prepared
            .connect(|base| {
                bases.lock().unwrap().push(base.bases());
                Ok::<_, ()>(script.clone())
            })
            .unwrap();
        Self {
            config,
            connected,
            bases,
            store: healthy(),
            keyring: MemoryKeyring::new(8),
            script,
            now: Cell::new(at()),
        }
    }

    fn answer(&self, headers: &[(&str, &str)], body: &[u8]) -> Answered {
        self.request(&Inbound {
            method: "POST",
            path: PATH,
            query: None,
            headers,
            body,
        })
    }

    fn request(&self, inbound: &Inbound<'_>) -> Answered {
        self.serving(|serving| pollster::block_on(answer(serving, inbound)))
    }

    /// What `job` did, worked as the server works it.
    fn work(&self, job: Job) -> Did {
        let worked = self.serving(|serving| pollster::block_on(work(serving, job, "server")));
        assert_eq!(worked.unreleased, None);
        worked.did
    }

    fn serving<T>(
        &self,
        serve: impl FnOnce(
            &Serving<
                '_,
                Failing,
                FakeClock,
                FakeSigner,
                FakeVerifier,
                Script,
                Script,
                MemoryKeyring,
            >,
        ) -> T,
    ) -> T {
        let clock = FakeClock::at(self.now.get());
        let ports = Ports {
            store: &self.store,
            clock: &clock,
            signer: &FakeSigner,
            budget: herald_flows::Budget::UNBOUNDED,
        };
        let keys = keys_in(&self.keyring);
        let webhooks = Webhooks::default();
        let serving = Serving {
            config: &self.config,
            connected: &self.connected,
            ports: &ports,
            verifier: &FakeVerifier,
            public: &self.script,
            keys: &keys,
            webhooks: &webhooks,
            admission: herald_flows::Admission::Once,
            merging: None,
        };
        serve(&serving)
    }

    /// The answer to `body` posted on `token`.
    fn post(&self, token: &str, body: &[u8]) -> Answered {
        let authorization = format!("Bearer {token}");
        self.answer(&[("Authorization", &authorization)], body)
    }

    fn made(&self) -> usize {
        self.script
            .made()
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// The status `answered` gives and why it was refused, where it was; which
/// neither its body nor its reason quotes `token` with, and which hands on
/// no work.
fn refused(answered: &Answered, token: &str) -> (u16, String) {
    assert!(answered.job.is_none());
    let (status, _, body) = said(answered);
    assert!(!body.contains(token), "{body}");
    let why = answered.refused.clone().unwrap_or_default();
    assert!(!why.contains(token), "{why}");
    assert!(
        token.get(..20).is_none_or(|start| !why.contains(start)),
        "{why}"
    );
    (status, why)
}

fn taken(answered: &Answered) {
    assert_eq!(
        (said(answered), &answered.refused, &answered.job),
        ((200, "application/json", "{}".to_owned()), &None, &None)
    );
}

#[test]
fn an_activity_vouched_for_by_a_key_endorsed_for_teams_is_taken_once_and_its_keys_kept() {
    let (teams, speech) = (pair(1), pair(2));
    let bot = Bot::new(publishing(&teams, &speech));
    let signed = token(&header("k1"), &claims(at()), &teams);
    taken(&bot.post(&signed, &activity("message", ACTIVITY_ID, SERVICE)));
    assert_eq!(
        bot.script.seen(),
        [format!("GET {METADATA}"), format!("GET {KEYS}")]
    );
    assert_eq!(bot.script.most(), [4 * 1024 * 1024, 4 * 1024 * 1024]);
    taken(&bot.post(&signed, &activity("message", ACTIVITY_ID, SERVICE)));
    taken(&bot.post(&signed, &activity("conversationUpdate", "2", SERVICE)));
    assert_eq!(bot.made(), 2);
    assert_eq!(
        pollster::block_on(bot.store.inner.admit(
            "teams",
            &herald_core::digest(&format!("19:abc@thread.tacv2\n{ACTIVITY_ID}")),
            at()
        )),
        Ok(herald_ports::Admitted::Seen)
    );
    bot.now.set(at() + SignedDuration::from_mins(6));
    let known = token(&header("k1"), &claims(bot.now.get()), &teams);
    taken(&bot.post(&known, &activity("message", "4", SERVICE)));
    let elsewhere = token(&header("k2"), &claims(bot.now.get()), &speech);
    assert_eq!(refused(&bot.post(&elsewhere, b"{}"), &elsewhere).0, 403);
    assert_eq!(bot.made(), 2);
    bot.now.set(at() + SignedDuration::from_hours(24));
    let later = token(&header("k1"), &claims(bot.now.get()), &teams);
    taken(&bot.post(&later, &activity("message", "3", SERVICE)));
    assert_eq!(bot.made(), 4);
}

#[test]
fn a_token_that_does_not_hold_is_refused_before_the_body_is_read() {
    let (teams, speech) = (pair(1), pair(2));
    let bot = Bot::new(publishing(&teams, &speech));
    let unread = b"this is no activity";
    let now = at().as_second();
    let with = |key: &str, value: Value| {
        let mut claims = claims(at());
        claims[key] = value;
        claims
    };
    let unvouched = |why: &str| (401, format!("the request's bearer token {why}"));
    for (claims, why) in [
        (
            with("iss", json!("https://api.botframework.com")),
            "names another issuer",
        ),
        (
            with("aud", json!("ffffffff-4e5f-6071-8293-a4b5c6d7e8f9")),
            "is meant for another audience",
        ),
        (with("exp", json!(now - 301)), "has expired"),
        (with("nbf", json!(now + 301)), "is not valid yet"),
        (
            with("exp", json!("later")),
            "names a time that is not a whole number of seconds since 1970",
        ),
    ] {
        let signed = token(&header("k1"), &claims, &teams);
        assert_eq!(
            refused(&bot.post(&signed, unread), &signed),
            unvouched(why),
            "{claims}"
        );
    }
    let mut unexpiring = claims(at());
    unexpiring.as_object_mut().unwrap().remove("exp");
    let signed = token(&header("k1"), &unexpiring, &teams);
    assert_eq!(
        refused(&bot.post(&signed, unread), &signed),
        unvouched("names no expiry")
    );
    let hmac = token(
        &json!({ "alg": "HS256", "kid": "k1" }),
        &claims(at()),
        &teams,
    );
    assert_eq!(
        refused(&bot.post(&hmac, unread), &hmac),
        unvouched("is signed with an algorithm herald does not check")
    );
    for unreadable in ["a.b", "not-a-token", "e30.e30.e30"] {
        assert_eq!(
            refused(&bot.post(unreadable, unread), unreadable),
            unvouched("cannot be read")
        );
    }
    let signed = token(&header("k1"), &claims(at()), &teams);
    for headers in [
        vec![],
        vec![("Authorization", format!("Basic {signed}"))],
        vec![("X-Authorization", format!("Bearer {signed}"))],
    ] {
        let headers: Vec<(&str, &str)> = headers
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        assert_eq!(
            refused(&bot.answer(&headers, unread), &signed),
            unvouched("is missing")
        );
    }
    assert_eq!(bot.made(), 0);
    let skewed = with("exp", json!(now - 299));
    let signed = token(&header("k1"), &skewed, &teams);
    let lower = format!("bearer {signed}");
    taken(&bot.answer(
        &[("authorization", &lower)],
        &activity("message", "9", SERVICE),
    ));
}

#[test]
fn a_token_signed_by_another_key_or_one_not_endorsed_for_teams_is_refused() {
    let (teams, speech, stranger) = (pair(1), pair(2), pair(3));
    let bot = Bot::new(publishing(&teams, &speech));
    let unread = b"this is no activity";
    let forged = token(&header("k1"), &claims(at()), &stranger);
    assert_eq!(
        refused(&bot.post(&forged, unread), &forged),
        (
            401,
            "the request's signature is not its sender's".to_owned()
        )
    );
    let endorsed_elsewhere = token(&header("k2"), &claims(at()), &speech);
    assert_eq!(
        refused(&bot.post(&endorsed_elsewhere, unread), &endorsed_elsewhere),
        (
            403,
            "the request's bearer token is signed by a key its issuer does not endorse for what herald takes"
                .to_owned()
        )
    );
    assert_eq!(bot.made(), 2);
    let unknown = token(&header("k9"), &claims(at()), &teams);
    let unkeyed = (
        401,
        "the request's bearer token names no key its issuer publishes".to_owned(),
    );
    assert_eq!(refused(&bot.post(&unknown, unread), &unknown), unkeyed);
    assert_eq!(refused(&bot.post(&unknown, unread), &unknown), unkeyed);
    assert_eq!(bot.made(), 2);
    let unnamed = token(&json!({ "alg": "RS256" }), &claims(at()), &teams);
    assert_eq!(refused(&bot.post(&unnamed, unread), &unnamed), unkeyed);
    let only = Bot::new(published(
        answered(200, &metadata(ISSUER, KEYS, &["RS256"])),
        answered(200, &key_set(&[("k1", &teams, &["msteams"])])),
    ));
    assert_eq!(refused(&only.post(&unnamed, unread), &unnamed), unkeyed);
    assert_eq!(only.made(), 0);
    bot.now
        .set(at() + SignedDuration::from_mins(5) - SignedDuration::from_secs(1));
    let early = token(&header("k9"), &claims(bot.now.get()), &teams);
    assert_eq!(refused(&bot.post(&early, unread), &early), unkeyed);
    assert_eq!(bot.made(), 2);
    bot.now.set(at() + SignedDuration::from_mins(5));
    let later = token(&header("k9"), &claims(bot.now.get()), &teams);
    assert_eq!(refused(&bot.post(&later, unread), &later), unkeyed);
    assert_eq!(bot.made(), 4);
}

#[test]
fn an_activity_naming_another_service_than_its_token_or_another_origin_sends_nothing_anywhere() {
    let (teams, speech) = (pair(1), pair(2));
    let bot = Bot::new(publishing(&teams, &speech));
    let attacker = "https://attacker.example/teams/";
    let signed = token(&header("k1"), &claims(at()), &teams);
    assert_eq!(
        refused(
            &bot.post(&signed, &commanded_at("1", "latest", attacker)),
            &signed
        ),
        (
            401,
            "the activity names a service URL its token does not vouch for".to_owned()
        )
    );
    let mut theirs = claims(at());
    theirs["serviceUrl"] = json!(attacker);
    let vouched = token(&header("k1"), &theirs, &teams);
    assert_eq!(
        refused(
            &bot.post(&vouched, &commanded_at("2", "latest", attacker)),
            &vouched
        ),
        (
            401,
            "the activity names a service URL that is not an https URL on the bot's service origin"
                .to_owned()
        )
    );
    let mut unnamed = claims(at());
    unnamed.as_object_mut().unwrap().remove("serviceUrl");
    let silent = token(&header("k1"), &unnamed, &teams);
    assert_eq!(
        refused(&bot.post(&silent, &commanded("3", "latest")), &silent),
        (
            401,
            "the request's bearer token carries no claim its request is read with".to_owned()
        )
    );
    assert_eq!(
        bot.script.seen(),
        [format!("GET {METADATA}"), format!("GET {KEYS}")]
    );
    assert!(
        bot.script
            .asked()
            .iter()
            .flatten()
            .all(|(name, _)| !name.eq_ignore_ascii_case("authorization"))
    );
}

#[test]
fn a_bot_takes_nothing_pushed_to_it_as_an_application_service() {
    let (teams, speech) = (pair(1), pair(2));
    let bot = Bot::new(publishing(&teams, &speech));
    let signed = token(&header("k1"), &claims(at()), &teams);
    let authorization = format!("Bearer {signed}");
    let pushed = bot.request(&Inbound {
        method: "PUT",
        path: "/matrix/teams/_matrix/app/v1/transactions/1",
        query: None,
        headers: &[("Authorization", &authorization)],
        body: &activity("message", "1", SERVICE),
    });
    assert_eq!(said(&pushed).0, 404);
    assert_eq!(bot.made(), 0);
}

#[test]
fn an_oversized_body_is_refused_before_its_token_or_any_key_is_read() {
    let (teams, speech) = (pair(1), pair(2));
    let bot = Bot::new(publishing(&teams, &speech));
    let signed = token(&header("k1"), &claims(at()), &teams);
    let large = vec![b' '; herald_dest_teams::ACTIVITY + 1];
    assert_eq!(
        refused(&bot.post(&signed, &large), &signed),
        (
            413,
            "the request's body is larger than its destination reads".to_owned()
        )
    );
    assert_eq!(refused(&bot.answer(&[], &large), &signed).0, 413);
    assert_eq!(bot.made(), 0);
    let mut fitting = activity("message", "1", SERVICE);
    fitting.resize(herald_dest_teams::ACTIVITY, b' ');
    taken(&bot.post(&signed, &fitting));
}

#[test]
fn keys_that_cannot_be_read_from_the_bot_framework_s_own_documents_take_nothing() {
    let (teams, speech) = (pair(1), pair(2));
    let keys = key_set(&[("k1", &teams, &["msteams"]), ("k2", &speech, &["webchat"])]);
    let signed = token(&header("k1"), &claims(at()), &teams);
    let body = activity("message", "1", SERVICE);
    for (script, why) in [
        (
            published(answered(500, ""), answered(200, &keys)),
            format!("{METADATA} answered 500"),
        ),
        (
            published(
                answered(
                    200,
                    &metadata("https://api.botframework.com", KEYS, &["RS256"]),
                ),
                answered(200, &keys),
            ),
            "the discovery document names another issuer".to_owned(),
        ),
        (
            published(
                answered(
                    200,
                    &metadata(ISSUER, "https://keys.example/keys", &["RS256"]),
                ),
                answered(200, &keys),
            ),
            "the discovery document's jwks_uri is not https on the issuer's own origin".to_owned(),
        ),
        (
            published(
                answered(200, &metadata(ISSUER, KEYS, &["RS512"])),
                answered(200, &keys),
            ),
            format!("{METADATA} lists no signing algorithm herald checks"),
        ),
        (
            published(
                answered(200, &metadata(ISSUER, KEYS, &["RS256"])),
                answered(200, &key_set(&[("k2", &speech, &["webchat"])])),
            ),
            format!("{KEYS} lists no key endorsed for msteams"),
        ),
        (
            published(
                answered(200, &metadata(ISSUER, KEYS, &["RS256"])),
                answered(404, ""),
            ),
            format!("{KEYS} answered 404"),
        ),
    ] {
        let bot = Bot::new(script);
        assert_eq!(
            refused(&bot.post(&signed, &body), &signed),
            (
                503,
                format!(
                    "the keys activities are checked with could not be read or kept, so the activity is not let through: the token's issuer's keys could not be read: {why}"
                )
            )
        );
    }
}

#[test]
fn an_activity_of_another_channel_or_a_type_the_bot_does_not_take_is_refused() {
    let (teams, speech) = (pair(1), pair(2));
    let bot = Bot::new(publishing(&teams, &speech));
    let signed = token(&header("k1"), &claims(at()), &teams);
    let mut webchat: Value = serde_json::from_slice(&activity("message", "1", SERVICE)).unwrap();
    webchat["channelId"] = json!("webchat");
    assert_eq!(
        refused(&bot.post(&signed, webchat.to_string().as_bytes()), &signed),
        (400, "the activity is not from Microsoft Teams".to_owned())
    );
    assert_eq!(
        refused(
            &bot.post(&signed, &activity("invoke", "2", SERVICE)),
            &signed
        ),
        (400, "the bot takes no activity of this type".to_owned())
    );
    assert_eq!(
        refused(&bot.post(&signed, b"[]"), &signed),
        (
            400,
            "the request is not a Bot Framework activity".to_owned()
        )
    );
}

const SIGN_IN: &str = "https://signin.example/botframework.com/oauth2/v2.0/token";

/// The activity of a member giving the bot `text` after its mention.
fn commanded(id: &str, text: &str) -> Vec<u8> {
    commanded_at(id, text, SERVICE)
}

/// The activity of a member giving the bot `text` after its mention,
/// naming `service`.
fn commanded_at(id: &str, text: &str, service: &str) -> Vec<u8> {
    let mut given: Value = serde_json::from_slice(&activity("message", id, service)).unwrap();
    given["text"] = json!(format!("<at>herald</at> {text}"));
    given["entities"] = json!([{
        "type": "mention",
        "text": "<at>herald</at>",
        "mentioned": { "id": format!("28:{APP}"), "name": "herald" },
    }]);
    given.to_string().into_bytes()
}

fn replied_at(id: &str) -> String {
    format!("https://smba.example/emea/v3/conversations/19%3Aabc%40thread.tacv2/activities/{id}")
}

/// The Bot Framework publishing `teams`, the sign-in giving `token`, and
/// a reply to the activity `id` answered `reply`.
fn answering(teams: &Pair, token: &str, id: &str, reply: Response) -> Script {
    Script::keyed(BTreeMap::from([
        (
            METADATA.to_owned(),
            answered(200, &metadata(ISSUER, KEYS, &["RS256"])),
        ),
        (
            KEYS.to_owned(),
            answered(200, &key_set(&[("k1", teams, &["msteams"])])),
        ),
        (
            SIGN_IN.to_owned(),
            answered(
                200,
                &json!({ "access_token": token, "expires_in": 3600 }).to_string(),
            ),
        ),
        (replied_at(id), reply),
    ]))
}

/// What the job `answered` handed on did, where it handed on one command.
fn interacted(bot: &Bot, answered: &Answered) -> herald_flows::Interacted {
    assert_eq!(said(answered), (200, "application/json", "{}".to_owned()));
    let job = answered.job.clone().unwrap();
    let Did::Interacted(mut interacted) = bot.work(job) else {
        panic!("the job answered no command");
    };
    assert_eq!(interacted.len(), 1);
    interacted.remove(0)
}

#[test]
fn a_command_is_answered_by_a_reply_carrying_the_bot_s_token_to_the_service_its_activity_names_alone()
 {
    let teams = pair(1);
    let token = ["made", "-", "at", "-", "run", "-", "time"].concat();
    let bot = Bot::new(answering(
        &teams,
        &token,
        "7",
        answered(201, r#"{"id":"1481567603899"}"#),
    ));
    let signed = token_of(&teams);
    let given = interacted(&bot, &bot.post(&signed, &commanded("7", "latest")));
    assert_eq!(
        (given.asked.user.as_str(), given.asked.command),
        (
            "00000000-0000-0000-0000-000000000001",
            herald_core::Command::Latest
        )
    );
    assert_eq!((given.sent, given.answer.trouble), (Ok(()), None));
    assert_eq!(
        bot.script.seen(),
        [
            format!("GET {METADATA}"),
            format!("GET {KEYS}"),
            format!("POST {SIGN_IN}"),
            format!("POST {}", replied_at("7")),
        ]
    );
    let asked = bot.script.asked();
    let authorised: Vec<bool> = asked
        .iter()
        .map(|headers| {
            headers
                .iter()
                .any(|(name, value)| name == "Authorization" && value.contains(&token))
        })
        .collect();
    assert_eq!(authorised, [false, false, false, true]);
    assert!(
        bot.script
            .bodies()
            .iter()
            .all(|body| !String::from_utf8_lossy(body).contains(&token))
    );
    assert!(bot.bases.lock().unwrap().contains(&Some(vec![
        "https://signin.example".to_owned(),
        "https://smba.example".to_owned(),
    ])));
    let replay = bot.post(&signed, &commanded("7", "latest"));
    taken(&replay);
    let again = interacted(&bot, &bot.post(&signed, &commanded("8", "changelog")));
    assert!(again.sent.is_err());
    assert_eq!(
        bot.script.seen().last(),
        Some(&format!("POST {}", replied_at("8")))
    );
    assert_eq!(
        bot.script
            .seen()
            .iter()
            .filter(|line| line.contains(SIGN_IN))
            .count(),
        1
    );
}

#[test]
fn a_reply_answered_by_a_redirect_is_not_followed_and_its_token_goes_nowhere_else() {
    let teams = pair(1);
    let token = ["made", "-", "at", "-", "run", "-", "time"].concat();
    let redirect = Response {
        status: 307,
        headers: vec![(
            "Location".to_owned(),
            "https://attacker.example/v3/conversations/a/activities/1".to_owned(),
        )],
        body: Vec::new(),
    };
    let bot = Bot::new(answering(&teams, &token, "7", redirect));
    let signed = token_of(&teams);
    let given = interacted(
        &bot,
        &bot.post(&signed, &commanded("7", "latest <at>x</at>")),
    );
    assert_eq!(given.sent, Err("Microsoft Teams answered 307".to_owned()));
    let seen = bot.script.seen();
    assert_eq!(seen.last(), Some(&format!("POST {}", replied_at("7"))));
    assert!(
        seen.iter().all(|line| !line.contains("attacker")),
        "{seen:?}"
    );
}

#[test]
fn a_command_the_sign_in_refuses_goes_unanswered_and_says_only_the_sign_in_s_error() {
    let teams = pair(1);
    let mut script = BTreeMap::from([
        (
            METADATA.to_owned(),
            answered(200, &metadata(ISSUER, KEYS, &["RS256"])),
        ),
        (
            KEYS.to_owned(),
            answered(200, &key_set(&[("k1", &teams, &["msteams"])])),
        ),
    ]);
    script.insert(
        SIGN_IN.to_owned(),
        answered(
            401,
            r#"{"error":"invalid_client","error_codes":[7000215],"error_description":"subscribe herald"}"#,
        ),
    );
    let bot = Bot::new(Script::keyed(script));
    let signed = token_of(&teams);
    let given = interacted(
        &bot,
        &bot.post(&signed, &commanded("7", "subscribe herald")),
    );
    let unsent = given.sent.unwrap_err();
    assert!(
        unsent.contains("invalid_client (AADSTS7000215)"),
        "{unsent}"
    );
    assert!(!unsent.contains("subscribe herald"), "{unsent}");
    assert_eq!(bot.script.seen().last(), Some(&format!("POST {SIGN_IN}")));
}

fn token_of(teams: &Pair) -> String {
    token(&header("k1"), &claims(at()), teams)
}
