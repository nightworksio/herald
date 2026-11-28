//! A CI run's ID token as an app's credential: taken where an entry
//! naming its issuer exactly holds it, its signature checked by a key of
//! the issuer's read once and kept for `keys_for`, and refused, never
//! quoted, where anything of it does not hold.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use herald_core::{IdToken, KeptKeys, KeptToken};
use herald_ports::{Admitted, Tokens as _};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

pub(crate) const ISSUER: &str = "https://issuer.example";
pub(crate) const DISCOVERED: &str = "GET https://issuer.example/.well-known/openid-configuration";
pub(crate) const KEYS: &str = "GET https://issuer.example/keys";
pub(crate) const AUDIENCE: &str = "https://herald.example";
const MODULUS: [u8; 4] = [0xc1, 2, 3, 4];
const EXPONENT: [u8; 3] = [1, 0, 1];
const DEPLOYED: &[u8] = br#"{"id": "7", "project": "herald", "title": "Deployed"}"#;
const EVENTS: &str = "/api/projects/herald/events";

/// A request's line, its answer's status and its body.
type Answer = (&'static str, u16, String);

fn encoded(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// An ID token of `claims`, its header `header`, signed as the fake
/// verifier takes a signature by the issuer's key.
pub(crate) fn id_token(header: &Value, claims: &Value) -> String {
    let signed = format!(
        "{}.{}",
        encoded(&serde_json::to_vec(header).unwrap()),
        encoded(&serde_json::to_vec(claims).unwrap())
    );
    let key = herald_keys::rsa_public(&MODULUS, &EXPONENT);
    let signature = [b"RsaSha256".as_slice(), &key, signed.as_bytes()].join(&b'|');
    format!("{signed}.{}", encoded(&signature))
}

/// How many runs' claims were made, so each names its own `jti`.
static RUNS: AtomicUsize = AtomicUsize::new(0);

/// The claims of a run of the repository `id`, issued now, under a `jti`
/// no other run's claims name.
pub(crate) fn run(id: &str) -> Value {
    let now = at().as_second();
    json!({
        "iss": ISSUER,
        "aud": AUDIENCE,
        "exp": now + 300,
        "iat": now - 10,
        "jti": format!("run-{}", RUNS.fetch_add(1, Ordering::Relaxed)),
        "repository": "org/plugin",
        "repository_id": id,
    })
}

pub(crate) fn header(kid: &str) -> Value {
    json!({ "alg": "RS256", "kid": kid })
}

/// The issuer's discovery document, naming its keys on its own origin.
pub(crate) fn discovery() -> String {
    json!({ "issuer": ISSUER, "jwks_uri": format!("{ISSUER}/keys") }).to_string()
}

/// The issuer's key set, holding its key under each id of `kids`.
pub(crate) fn key_set(kids: &[&str]) -> String {
    let keys: Vec<Value> = kids
        .iter()
        .map(|kid| json!({ "kty": "RSA", "kid": kid, "n": encoded(&MODULUS), "e": encoded(&EXPONENT) }))
        .collect();
    json!({ "keys": keys }).to_string()
}

/// The config of two apps taking the issuer's tokens, each entry reading
/// its keys as `keys` says.
fn config(keys: &str) -> Config {
    let text = config_text().replace(
        "sources = [\"ci\"]",
        "sources = [\"ci\", \"runs\", \"late\"]",
    ) + &format!(
        "[apps.runs]\nprojects = [\"herald\"]\noidc = [\
             {{ issuer = \"{ISSUER}\", audience = \"{AUDIENCE}\", claims = {{ repository_id = \"42\", repository = \"org/*\" }}{keys} }}, \
             {{ issuer = \"{ISSUER}\", audience = \"{AUDIENCE}\", claims = {{ repository_id = \"43\" }}{keys} }}]\n\
             [apps.late]\nprojects = [\"herald\"]\noidc = [{{ issuer = \"{ISSUER}\", audience = \"https://late.example\", claims = {{ repository_id = \"44\" }}{keys} }}]\n"
    );
    load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

fn world(answers: &[(&str, u16, &str)]) -> World {
    World::with(config(""), Script::answering(answers))
}

fn fetched() -> [(&'static str, u16, String); 2] {
    [
        (DISCOVERED, 200, discovery()),
        (KEYS, 200, key_set(&["k1"])),
    ]
}

fn answering<'a>(answers: &'a [(&'a str, u16, String)]) -> Vec<(&'a str, u16, &'a str)> {
    answers
        .iter()
        .map(|(line, status, body)| (*line, *status, body.as_str()))
        .collect()
}

fn post(world: &World, token: &str, id: &str) -> Answered {
    let authorization = format!("Bearer {token}");
    let body = DEPLOYED.to_vec();
    let body = String::from_utf8(body)
        .unwrap()
        .replace("\"7\"", &format!("\"{id}\""));
    world.answer(&Inbound {
        method: "POST",
        path: EVENTS,
        query: None,
        headers: &[("Authorization", &authorization)],
        body: body.as_bytes(),
    })
}

/// The status `answered` gives and why, which neither its body nor its
/// reason quotes `token` with.
fn refused(answered: &Answered, token: &str) -> (u16, String) {
    let (status, _, body) = said(answered);
    assert!(!body.contains(token), "{body}");
    let why = answered.refused.clone().unwrap_or_default();
    assert!(!why.contains(token), "{why}");
    (status, why)
}

fn kept(world: &World) -> Option<KeptToken> {
    let name = format!("oidc/{}", herald_core::digest(ISSUER));
    pollster::block_on(world.store.inner.token(&name))
        .unwrap()
        .map(|kept| kept.value)
}

#[test]
fn a_run_s_id_token_is_taken_by_the_first_entry_it_matches_and_its_keys_kept() {
    let world = world(&answering(&fetched()));
    let first = post(&world, &id_token(&header("k1"), &run("42")), "1");
    assert_eq!(said(&first).0, 202, "{:?}", first.refused);
    let Some(Job::App { app, verified, .. }) = first.job else {
        panic!("the event is left to announce");
    };
    assert_eq!(app, "runs");
    assert_eq!(verified.signer.as_deref(), Some("runs"));
    assert_eq!(world.script.seen(), [DISCOVERED, KEYS]);
    assert_eq!(world.script.most(), [64 * 1024, 64 * 1024]);
    let kept = kept(&world).unwrap();
    assert_eq!(kept.expires, at() + jiff::SignedDuration::from_hours(1));
    let unread = World::with(config(""), Script::answering(&[(DISCOVERED, 404, "")]));
    let untried = id_token(&header("k1"), &run("42"));
    assert_eq!(refused(&post(&unread, &untried, "1"), &untried).0, 503);
    assert_eq!(
        self::kept(&unread).unwrap().expires,
        at() + jiff::SignedDuration::from_hours(1)
    );
    let second = post(&world, &id_token(&header("k1"), &run("43")), "2");
    assert_eq!(said(&second).0, 202, "{:?}", second.refused);
    let mut late = run("44");
    late["aud"] = json!("https://late.example");
    let Some(Job::App { app, .. }) = post(&world, &id_token(&header("k1"), &late), "3").job else {
        panic!("the late app takes its run");
    };
    assert_eq!(app, "late");
    assert_eq!(world.script.seen().len(), 2);
}

#[test]
fn a_token_no_entry_holds_is_refused_before_any_key_is_read() {
    let world = world(&[]);
    let mut stranger = run("42");
    stranger["iss"] = json!("https://issuer.example/");
    let mut unissued = run("42");
    unissued.as_object_mut().unwrap().remove("iss");
    let mut expired = run("42");
    expired["exp"] = json!(at().as_second() - 3600);
    let mut elsewhere = run("42");
    elsewhere["aud"] = json!("https://elsewhere.example");
    let cases = [
        ("a.b.c".to_owned(), "the ID token cannot be read"),
        (
            id_token(&header("k1"), &stranger),
            "no app takes ID tokens of the token's issuer",
        ),
        (
            id_token(&header("k1"), &unissued),
            "no app takes ID tokens of the token's issuer",
        ),
        (
            id_token(&json!({ "alg": "HS256", "kid": "k1" }), &run("42")),
            "the ID token is signed with an algorithm its issuer's entry does not allow",
        ),
        (
            id_token(&json!({ "alg": "none" }), &run("42")),
            "the ID token is signed with an algorithm its issuer's entry does not allow",
        ),
        (
            id_token(&header("k1"), &expired),
            "the ID token has expired",
        ),
        (
            id_token(&header("k1"), &elsewhere),
            "the ID token is not meant for this herald",
        ),
    ];
    for (token, why) in cases {
        let answered = post(&world, &token, "1");
        assert_eq!(refused(&answered, &token), (401, why.to_owned()), "{why}");
        assert_eq!(answered.job, None);
    }
    assert_eq!(world.script.seen(), Vec::<String>::new());
}

#[test]
fn a_token_whose_signature_or_claims_do_not_hold_is_refused() {
    let world = world(&answering(&fetched()));
    let forged = format!(
        "{}.{}",
        id_token(&header("k1"), &run("42"))
            .rsplit_once('.')
            .unwrap()
            .0,
        encoded(b"forged")
    );
    let mut evil = run("42");
    evil["repository"] = json!("org-evil/x");
    let mut deep = run("42");
    deep["repository"] = json!("org/a/b");
    for (token, why) in [
        (forged, "the ID token's signature does not hold"),
        (
            id_token(&header("k1"), &run("45")),
            "the ID token's claim repository_id does not match its app's entry",
        ),
        (
            id_token(&header("k1"), &evil),
            "the ID token's claim repository does not match its app's entry",
        ),
        (
            id_token(&header("k1"), &deep),
            "the ID token's claim repository does not match its app's entry",
        ),
    ] {
        assert_eq!(
            refused(&post(&world, &token, "1"), &token),
            (401, why.to_owned())
        );
    }
    let wide = id_token(&header("k1"), &run("42"));
    let answered = world.answer(&Inbound {
        method: "POST",
        path: "/api/projects/other/events",
        query: None,
        headers: &[("Authorization", &format!("Bearer {wide}"))],
        body: DEPLOYED,
    });
    assert_eq!(said(&answered).0, 403);
}

#[test]
fn unknown_keys_fetch_the_issuer_s_keys_at_most_once_a_cooldown_and_old_keys_fail_closed() {
    let answers: Vec<(&str, u16, String)> = [
        Some(key_set(&["k1"])),
        Some(key_set(&["k1", "k2"])),
        Some(key_set(&["k1", "k2"])),
        Some(key_set(&["k1", "k2", "k3"])),
    ]
    .into_iter()
    .flat_map(|set| {
        [(DISCOVERED, 200, discovery())]
            .into_iter()
            .chain(set.map(|set| (KEYS, 200, set)))
    })
    .chain([
        (DISCOVERED, 404, String::new()),
        (DISCOVERED, 404, String::new()),
    ])
    .collect();
    let world = world(&answering(&answers));
    let start = at();
    let posted = |kid: &str, after: i64, id: &str| {
        let now = start + jiff::SignedDuration::from_secs(after);
        world.at(now);
        let mut claims = run("42");
        claims["iat"] = json!(now.as_second() - 10);
        claims["exp"] = json!(now.as_second() + 300);
        let token = id_token(&header(kid), &claims);
        let answered = post(&world, &token, id);
        let (status, why) = refused(&answered, &token);
        assert!(
            status != 401 || why == "the ID token's issuer publishes no key it names",
            "{why}"
        );
        (status, world.script.seen().len())
    };
    assert_eq!(posted("k1", 0, "1"), (202, 2));
    assert_eq!(posted("k9", 0, "2"), (401, 2));
    assert_eq!(posted("k9", 59, "3"), (401, 2));
    assert_eq!(posted("k8", 60, "4"), (401, 4));
    assert_eq!(posted("k2", 61, "5"), (202, 4));
    assert_eq!(posted("k7", 130, "6"), (401, 6));
    assert_eq!(posted("k3", 140, "7"), (401, 6));
    assert_eq!(posted("k3", 190, "8"), (202, 8));
    assert_eq!(posted("k6", 250, "9"), (401, 9));
    assert_eq!(posted("k1", 260, "10"), (202, 9));
    assert_eq!(posted("k1", 190 + 3600, "11"), (503, 10));
    assert_eq!(posted("k1", 190 + 3610, "12"), (503, 10));
}

#[test]
fn keys_that_cannot_be_read_or_kept_leave_the_token_untaken_for_now() {
    let unread = "the token's issuer's keys could not be read: ";
    let other = json!({ "issuer": "https://other.example", "jwks_uri": format!("{ISSUER}/keys") })
        .to_string();
    let away = json!({ "issuer": ISSUER, "jwks_uri": "https://keys.example/keys" }).to_string();
    let cases: [(Vec<Answer>, String); 6] = [
        (
            vec![(DISCOVERED, 404, String::new())],
            format!("{unread}https://issuer.example/.well-known/openid-configuration answered 404"),
        ),
        (
            vec![(DISCOVERED, 200, other)],
            format!("{unread}the discovery document names another issuer"),
        ),
        (
            vec![(DISCOVERED, 200, away)],
            format!(
                "{unread}the discovery document's jwks_uri is not https on the issuer's own origin"
            ),
        ),
        (
            vec![(DISCOVERED, 200, discovery()), (KEYS, 200, key_set(&[]))],
            format!("{unread}https://issuer.example/keys lists no key herald checks"),
        ),
        (
            vec![(DISCOVERED, 200, discovery()), (KEYS, 200, "[]".to_owned())],
            format!("{unread}the key set is not a JSON Web Key Set"),
        ),
        (
            vec![(DISCOVERED, 200, "x".repeat(64 * 1024 + 1))],
            format!(
                "{unread}https://issuer.example/.well-known/openid-configuration could not be read: "
            ),
        ),
    ];
    let token = id_token(&header("k1"), &run("42"));
    for (answers, why) in cases {
        let world = world(&answering(&answers));
        let answered = post(&world, &token, "1");
        assert_eq!(said(&answered).2, "herald cannot answer this request now");
        let (status, body) = refused(&answered, &token);
        assert_eq!(status, 503, "{body}");
        assert!(body.starts_with(&why), "{body} is not {why}");
        let seen = world.script.seen().len();
        let (status, body) = refused(&post(&world, &token, "2"), &token);
        assert_eq!(status, 503, "{body}");
        assert_eq!(
            body,
            "the token's issuer's keys could not be read: the kept ones are older than keys_for, and they were fetched or tried within keys_retry"
        );
        assert_eq!(world.script.seen().len(), seen);
    }
    for fault in [Fault::ReadsTokens, Fault::WritesTokens] {
        let mut world = world(&answering(&fetched()));
        world.store.fault = fault;
        let (status, body) = refused(&post(&world, &token, "1"), &token);
        assert_eq!(status, 503, "{fault:?}");
        assert!(
            body.starts_with(&format!("{unread}the store could not keep them: ")),
            "{body}"
        );
    }
}

#[test]
fn keys_kept_unreadably_are_read_again_and_kept_anew() {
    let world = world(&answering(&fetched()));
    let name = format!("oidc/{}", herald_core::digest(ISSUER));
    let garbled = KeptToken {
        name: name.clone(),
        sealed: "not base64!".to_owned(),
        expires: at(),
    };
    pollster::block_on(world.store.inner.put_token(&garbled, None)).unwrap();
    assert_eq!(
        said(&post(&world, &id_token(&header("k1"), &run("42")), "1")).0,
        202
    );
    assert_eq!(world.script.seen(), [DISCOVERED, KEYS]);
    assert_ne!(kept(&world).unwrap().sealed, garbled.sealed);
}

#[test]
fn keys_named_in_the_config_are_read_there_or_never() {
    let jwks = config(", jwks = \"https://keys.example/set\"");
    let world = World::with(
        jwks,
        Script::answering(&[("GET https://keys.example/set", 200, &key_set(&["k1"]))]),
    );
    let token = id_token(&header("k1"), &run("42"));
    assert_eq!(said(&post(&world, &token, "1")).0, 202);
    assert_eq!(world.script.seen(), ["GET https://keys.example/set"]);
    let pinned = config(&format!(
        ", keys = [{{ kty = \"RSA\", kid = \"k1\", n = \"{}\", e = \"{}\" }}]",
        encoded(&MODULUS),
        encoded(&EXPONENT)
    ));
    let world = World::with(pinned, Script::default());
    let unnamed = id_token(&json!({ "alg": "RS256" }), &run("42"));
    assert_eq!(said(&post(&world, &unnamed, "1")).0, 202);
    assert_eq!(world.script.seen(), Vec::<String>::new());
}

/// The answer to `token` posting the event `id`, worked once more after it
/// was answered where `again`.
fn posted(world: &World, token: &str, (id, again): (&str, bool)) -> Answered {
    let authorization = format!("Bearer {token}");
    let body = String::from_utf8(DEPLOYED.to_vec())
        .unwrap()
        .replace("\"7\"", &format!("\"{id}\""));
    let inbound = Inbound {
        method: "POST",
        path: EVENTS,
        query: None,
        headers: &[("Authorization", &authorization)],
        body: body.as_bytes(),
    };
    if again {
        world.answer_again(&inbound)
    } else {
        world.answer(&inbound)
    }
}

#[test]
fn a_token_taken_once_is_refused_a_second_time_unless_its_request_is_worked_again() {
    let world = world(&answering(&fetched()));
    let mut named = run("42");
    named["jti"] = json!("run-1");
    let token = id_token(&header("k1"), &named);
    assert_eq!(said(&posted(&world, &token, ("1", false))).0, 202);
    let remembered = IdToken::read(&token).unwrap().remembered_as();
    assert_eq!(world.admits("oidc", &remembered), Admitted::Seen);
    let replayed = posted(&world, &token, ("2", false));
    assert_eq!(
        refused(&replayed, &token),
        (
            401,
            "the ID token was taken before, and each is taken once".to_owned()
        )
    );
    assert_eq!(replayed.job, None);
    let mut rerun = named.clone();
    rerun["exp"] = json!(at().as_second() + 200);
    let resigned = id_token(&header("k1"), &rerun);
    assert_eq!(
        refused(&posted(&world, &resigned, ("3", false)), &resigned).0,
        401
    );
    assert_eq!(said(&posted(&world, &token, ("4", true))).0, 202);
    let mut unidentified = run("43");
    unidentified.as_object_mut().unwrap().remove("jti");
    let unnamed = id_token(&header("k1"), &unidentified);
    assert_eq!(said(&posted(&world, &unnamed, ("5", false))).0, 202);
    assert_eq!(
        refused(&posted(&world, &unnamed, ("6", false)), &unnamed).0,
        401
    );
    assert_eq!(world.script.seen(), [DISCOVERED, KEYS]);
}

#[test]
fn a_token_whose_use_cannot_be_kept_is_not_taken_for_now() {
    let mut world = world(&answering(&fetched()));
    world.store.fault = Fault::Admits;
    let token = id_token(&header("k1"), &run("42"));
    let answered = post(&world, &token, "1");
    assert_eq!(said(&answered).2, "herald cannot answer this request now");
    let (status, why) = refused(&answered, &token);
    assert_eq!(status, 503);
    assert!(
        why.starts_with("whether the ID token was taken before could not be kept: "),
        "{why}"
    );
}

#[test]
fn an_app_taking_a_token_until_it_expires_takes_it_again() {
    let until = load(
        "herald.toml",
        &(config_text().replace("sources = [\"ci\"]", "sources = [\"ci\", \"runs\"]")
            + &format!(
                "[apps.runs]\nprojects = [\"herald\"]\nreplay = \"until-expiry\"\noidc = [{{ issuer = \"{ISSUER}\", audience = \"{AUDIENCE}\", claims = {{ repository_id = \"42\" }} }}]\n"
            )),
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let world = World::with(until, Script::answering(&answering(&fetched())));
    let token = id_token(&header("k1"), &run("42"));
    assert_eq!(said(&post(&world, &token, "1")).0, 202);
    assert_eq!(said(&post(&world, &token, "2")).0, 202);
}

#[test]
fn keys_kept_as_read_or_tried_later_than_now_are_read_again() {
    let world = world(&answering(&fetched()));
    let name = format!("oidc/{}", herald_core::digest(ISSUER));
    let ahead = at() + jiff::SignedDuration::from_hours(1);
    let kept = KeptKeys::new(key_set(&["k1"]).as_bytes(), ahead);
    pollster::block_on(world.store.inner.put_token(
        &kept.kept(name, ahead + jiff::SignedDuration::from_hours(1)),
        None,
    ))
    .unwrap();
    let token = id_token(&header("k1"), &run("42"));
    assert_eq!(said(&post(&world, &token, "1")).0, 202);
    assert_eq!(world.script.seen(), [DISCOVERED, KEYS]);
    assert_eq!(
        KeptKeys::read(&self::kept(&world).unwrap())
            .unwrap()
            .fetched,
        Some(at())
    );
}

#[test]
fn the_entry_meant_for_a_token_says_why_it_holds_it_not_before_any_other() {
    let world = world(&[]);
    let mut early = run("44");
    early["aud"] = json!("https://late.example");
    early["iat"] = json!(at().as_second() + 120);
    let token = id_token(&header("k1"), &early);
    assert_eq!(
        refused(&post(&world, &token, "1"), &token),
        (401, "the ID token was issued later than now".to_owned())
    );
    assert_eq!(world.script.seen(), Vec::<String>::new());
}
