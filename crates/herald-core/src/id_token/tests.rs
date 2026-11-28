use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::json;

use super::*;

const ISSUER: &str = "https://token.actions.githubusercontent.com";
const AUDIENCE: &str = "https://herald.example";
const NOW: i64 = 1_800_000_000;

fn part(value: &Value) -> String {
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).unwrap())
}

fn token(header: &Value, claims: &Value) -> String {
    format!(
        "{}.{}.{}",
        part(header),
        part(claims),
        URL_SAFE_NO_PAD.encode(b"sig")
    )
}

fn claims() -> Value {
    json!({
        "iss": ISSUER,
        "aud": AUDIENCE,
        "exp": NOW + 300,
        "nbf": NOW - 10,
        "iat": NOW - 10,
        "repository": "org/plugin",
        "repository_id": 42,
        "ref_protected": true,
        "steps": ["a"],
    })
}

fn read(claims: &Value) -> IdToken {
    IdToken::read(&token(&json!({ "alg": "RS256", "kid": "k1" }), claims)).unwrap()
}

fn now() -> Timestamp {
    Timestamp::from_second(NOW).unwrap()
}

#[test]
fn a_token_is_read_into_its_header_claims_signed_bytes_and_signature() {
    let text = token(&json!({ "alg": "RS256", "kid": "k1" }), &claims());
    let read = IdToken::read(&text).unwrap();
    assert_eq!(read.issuer(), Some(ISSUER));
    assert_eq!(read.key_id(), Some("k1"));
    assert_eq!(read.signature(), b"sig");
    assert_eq!(read.signed(), text.rsplit_once('.').unwrap().0.as_bytes());
    assert_eq!(
        read.algorithm(&[TokenAlgorithm::Rs256]),
        Ok(TokenAlgorithm::Rs256)
    );
    assert_eq!(read.algorithm(&[]), Err(Untaken::Algorithm));
    assert_eq!(
        read.valid().map(|(issued, _)| issued),
        Some(Timestamp::from_second(NOW - 10).unwrap())
    );
    let unnamed = IdToken::read(&token(&json!({ "alg": "RS256" }), &claims())).unwrap();
    assert_eq!(unnamed.key_id(), None);
    assert!(IdToken::shaped(&text));
    assert!(!IdToken::shaped("a.b"));
    assert!(!IdToken::shaped("a.b.c.d"));
    assert!(!IdToken::shaped("plain-token"));
}

#[test]
fn a_token_of_another_shape_or_algorithm_is_unreadable_or_refused() {
    let header = part(&json!({ "alg": "RS256" }));
    let body = part(&claims());
    let signature = URL_SAFE_NO_PAD.encode(b"sig");
    for text in [
        format!("{header}.{body}"),
        format!("{header}.{body}.{signature}.{signature}"),
        format!("{header}.{body}."),
        format!("{header}.{body}.!!"),
        format!("{}.{body}.{signature}", part(&json!(["alg"]))),
        format!("{}.{body}.{signature}", part(&json!({ "kid": "k" }))),
        format!("{}.{body}.{signature}", part(&json!({ "alg": 256 }))),
        format!(
            "{}.{body}.{signature}",
            part(&json!({ "alg": "RS256", "kid": 1 }))
        ),
        format!("{header}.{}.{signature}", part(&json!("claims"))),
        format!("{header}.not-json.{signature}"),
        format!("{header}.{body}.{}", "A".repeat(IdToken::MOST)),
    ] {
        assert_eq!(IdToken::read(&text), Err(Untaken::Unreadable), "{text}");
    }
    let longest = (0..IdToken::MOST)
        .flat_map(|pad| {
            let mut padded = claims();
            padded["pad"] = json!("x".repeat(pad));
            let body = part(&padded);
            [&b"sig"[..], b"sigs"]
                .map(|signature| format!("{header}.{body}.{}", URL_SAFE_NO_PAD.encode(signature)))
        })
        .find(|text| text.len() == IdToken::MOST)
        .unwrap();
    assert!(IdToken::read(&longest).is_ok());
    assert_eq!(IdToken::MOST, 16_384);
    assert_eq!(
        IdToken::read(&format!("{longest}A")),
        Err(Untaken::Unreadable)
    );
    for algorithm in ["none", "HS256", "rs256", "ES256"] {
        let named = IdToken::read(&token(&json!({ "alg": algorithm }), &claims())).unwrap();
        assert_eq!(
            named.algorithm(&TokenAlgorithm::ALL),
            Err(Untaken::Algorithm),
            "{algorithm}"
        );
    }
    assert_eq!(TokenAlgorithm::named("RS256"), Some(TokenAlgorithm::Rs256));
    assert_eq!(TokenAlgorithm::named("none"), None);
}

#[test]
fn a_token_is_held_to_its_audience_and_its_times_give_or_take_the_skew() {
    let bounds = (SignedDuration::from_secs(60), SignedDuration::from_hours(1));
    assert_eq!(read(&claims()).held_to(AUDIENCE, now(), bounds), Ok(()));
    let mut listed = claims();
    listed["aud"] = json!(["other", AUDIENCE]);
    assert_eq!(read(&listed).held_to(AUDIENCE, now(), bounds), Ok(()));
    for aud in [json!("other"), json!(["other"]), json!(7), Value::Null] {
        let mut other = claims();
        other["aud"] = aud;
        assert_eq!(
            read(&other).held_to(AUDIENCE, now(), bounds),
            Err(Untaken::Audience)
        );
    }
    let at = |name: &str, value: Value| {
        let mut changed = claims();
        changed[name] = value;
        read(&changed).held_to(AUDIENCE, now(), bounds)
    };
    let time = |why| Err(Untaken::Time(why));
    assert_eq!(at("exp", json!(NOW - 61)), time(Untaken::EXPIRED));
    assert_eq!(at("exp", json!(NOW - 60)), time(Untaken::EXPIRED));
    assert_eq!(at("exp", json!(NOW - 9)), Ok(()));
    assert_eq!(at("exp", json!(NOW - 11)), time(Untaken::BACKWARD));
    assert_eq!(at("exp", json!(NOW - 10)), Ok(()));
    assert_eq!(at("nbf", json!(NOW + 61)), time(Untaken::EARLY));
    assert_eq!(at("nbf", json!(NOW + 60)), Ok(()));
    assert_eq!(at("iat", json!(NOW + 61)), time(Untaken::AHEAD));
    assert_eq!(at("iat", json!(NOW + 60)), Ok(()));
    assert_eq!(at("iat", json!(NOW + 300 - 3600)), Ok(()));
    assert_eq!(at("iat", json!(NOW + 300 - 3601)), time(Untaken::LONG));
    for name in ["exp", "nbf", "iat"] {
        for unwhole in [
            json!(1.8e9),
            json!(f64::from(u32::MAX)),
            json!("1800000000"),
            json!(-1),
            json!(true),
            json!([NOW]),
            Value::Null,
        ] {
            assert_eq!(
                at(name, unwhole.clone()),
                time(Untaken::UNWHOLE),
                "{name} {unwhole}"
            );
        }
    }
    for (name, why) in [("exp", Untaken::NO_EXPIRY), ("iat", Untaken::NO_ISSUE)] {
        let mut absent = claims();
        absent.as_object_mut().unwrap().remove(name);
        assert_eq!(
            read(&absent).held_to(AUDIENCE, now(), bounds),
            time(why),
            "{name}"
        );
        assert_eq!(read(&absent).valid(), None, "{name}");
    }
    let mut unbounded = claims();
    unbounded.as_object_mut().unwrap().remove("nbf");
    assert_eq!(read(&unbounded).held_to(AUDIENCE, now(), bounds), Ok(()));
    let short = (
        SignedDuration::from_secs(60),
        SignedDuration::from_secs(310),
    );
    assert_eq!(read(&claims()).held_to(AUDIENCE, now(), short), Ok(()));
    let shorter = (
        SignedDuration::from_secs(60),
        SignedDuration::from_secs(309),
    );
    assert_eq!(
        read(&claims()).held_to(AUDIENCE, now(), shorter),
        time(Untaken::LONG)
    );
    let exact = (SignedDuration::ZERO, SignedDuration::from_hours(1));
    assert_eq!(
        read(&claims()).held_to(AUDIENCE, now() + SignedDuration::from_secs(299), exact),
        Ok(())
    );
    assert_eq!(
        read(&claims()).held_to(AUDIENCE, now() + SignedDuration::from_secs(300), exact),
        time(Untaken::EXPIRED)
    );
}

#[test]
fn a_token_is_remembered_by_its_issuer_and_id_or_its_signed_bytes_never_its_text() {
    let text = token(&json!({ "alg": "RS256", "kid": "k1" }), &claims());
    let unnamed = IdToken::read(&text).unwrap();
    let mut identified = claims();
    identified["jti"] = json!("run-1");
    let named = read(&identified);
    let issuer = digest(ISSUER);
    assert_eq!(
        named.remembered_as(),
        format!("{issuer}/{}", digest("jti\nrun-1"))
    );
    let (signed, _) = text.rsplit_once('.').unwrap();
    assert_eq!(
        unnamed.remembered_as(),
        format!(
            "{issuer}/{}",
            digest(&format!(
                "token\n{signed}\n{}",
                URL_SAFE_NO_PAD.encode(b"sig")
            ))
        )
    );
    assert_ne!(unnamed.remembered_as(), named.remembered_as());
    let mut elsewhere = identified.clone();
    elsewhere["iss"] = json!("https://gitlab.example");
    assert_ne!(read(&elsewhere).remembered_as(), named.remembered_as());
    let mut rerun = identified.clone();
    rerun["exp"] = json!(NOW + 600);
    assert_eq!(read(&rerun).remembered_as(), named.remembered_as());
    assert!(!named.remembered_as().contains("run-1"));
    let mut ran = identified.clone();
    ran["run_id"] = json!(7);
    ran["sha"] = json!("abc");
    ran["job_workflow_ref"] = json!("org/plugin/.github/workflows/ci.yml@refs/heads/main");
    ran["steps"] = json!(["a"]);
    assert_eq!(
        read(&ran).evidence(&BTreeMap::new()),
        BTreeMap::from([
            ("iss".to_owned(), ISSUER.to_owned()),
            ("jti".to_owned(), "run-1".to_owned()),
            ("run_id".to_owned(), "7".to_owned()),
            ("sha".to_owned(), "abc".to_owned()),
            (
                "job_workflow_ref".to_owned(),
                "org/plugin/.github/workflows/ci.yml@refs/heads/main".to_owned()
            ),
        ])
    );
    assert_eq!(
        named.valid(),
        Some((
            Timestamp::from_second(NOW - 10).unwrap(),
            Timestamp::from_second(NOW + 300).unwrap()
        ))
    );
}

#[test]
fn each_claim_an_entry_names_must_match_and_evidence_holds_the_issuer_and_those_claims() {
    let token = read(&claims());
    assert_eq!(token.claim("repository_id").as_deref(), Some("42"));
    assert_eq!(token.claim("ref_protected").as_deref(), Some("true"));
    assert_eq!(token.claim("steps"), None);
    assert_eq!(token.claim("absent"), None);
    let wanted = |pairs: &[(&str, &str)]| {
        pairs
            .iter()
            .map(|(name, pattern)| ((*name).to_owned(), ClaimPattern::new(*pattern)))
            .collect::<BTreeMap<_, _>>()
    };
    assert_eq!(token.matching(&wanted(&[])), Ok(()));
    assert_eq!(
        token.matching(&wanted(&[("repository", "org/*"), ("repository_id", "42")])),
        Ok(())
    );
    assert_eq!(
        token.matching(&wanted(&[("repository", "org/*"), ("repository_id", "43")])),
        Err(Untaken::Claim("repository_id".to_owned()))
    );
    assert_eq!(
        token.matching(&wanted(&[("workflow", "*")])),
        Err(Untaken::Claim("workflow".to_owned()))
    );
    let evidence = token.evidence(&wanted(&[
        ("repository", "org/*"),
        ("repository_id", "42"),
        ("steps", "*"),
        ("workflow", "*"),
    ]));
    assert_eq!(
        evidence,
        BTreeMap::from([
            ("iss".to_owned(), ISSUER.to_owned()),
            ("repository".to_owned(), "org/plugin".to_owned()),
            ("repository_id".to_owned(), "42".to_owned()),
        ])
    );
    let shown = format!("{token:?}");
    assert!(shown.contains(ISSUER) && shown.contains("k1"), "{shown}");
    assert!(!shown.contains(&URL_SAFE_NO_PAD.encode(b"sig")), "{shown}");
    assert!(!shown.contains("org/plugin"), "{shown}");
}

#[test]
fn a_key_set_keeps_only_rsa_signing_keys_and_finds_one_by_id() {
    let n = URL_SAFE_NO_PAD.encode([1, 2, 3]);
    let e = URL_SAFE_NO_PAD.encode([1, 0, 1]);
    let body = serde_json::to_vec(&json!({ "keys": [
        { "kty": "RSA", "kid": "k1", "n": n, "e": e, "alg": "RS256", "use": "sig" },
        { "kty": "RSA", "kid": "k2", "n": n, "e": e },
        { "kty": "RSA", "kid": "enc", "n": n, "e": e, "use": "enc" },
        { "kty": "RSA", "kid": "rs512", "n": n, "e": e, "alg": "RS512" },
        { "kty": "EC", "kid": "ec", "crv": "P-256" },
        { "kty": "RSA", "kid": "bad", "n": "!!", "e": e },
        { "kty": "RSA", "kid": "short", "n": n },
        "not a key",
    ] }))
    .unwrap();
    let set = KeySet::read(&body).unwrap();
    assert_eq!(set.key(Some("k1")).unwrap().modulus, [1, 2, 3]);
    assert_eq!(set.key(Some("k2")).unwrap().exponent, [1, 0, 1]);
    for absent in ["enc", "rs512", "ec", "bad", "short", "k3"] {
        assert_eq!(set.key(Some(absent)), None, "{absent}");
    }
    assert_eq!(set.key(None), None);
    assert!(!set.is_empty());
    let one = KeySet::read(br#"{"keys":[{"kty":"RSA","n":"AQ","e":"AQAB"}]}"#).unwrap();
    assert_eq!(one.key(None).unwrap().id, None);
    assert!(KeySet::read(br#"{"keys":[]}"#).unwrap().is_empty());
    for unread in [&b"[]"[..], b"{}", br#"{"keys":{}}"#, b"no"] {
        assert!(matches!(KeySet::read(unread), Err(Untaken::Keys(_))));
    }
}

#[test]
fn discovery_names_the_issuer_s_own_key_set_on_its_origin() {
    let document = |issuer: &str, jwks: &str| {
        serde_json::to_vec(&json!({ "issuer": issuer, "jwks_uri": jwks })).unwrap()
    };
    let own = format!("{ISSUER}/.well-known/jwks");
    assert_eq!(jwks_uri(&document(ISSUER, &own), ISSUER), Ok(own.clone()));
    for (body, why) in [
        (
            b"[]".to_vec(),
            "the discovery document is not a JSON object",
        ),
        (
            document("https://evil.example", &own),
            "the discovery document names another issuer",
        ),
        (
            serde_json::to_vec(&json!({ "issuer": ISSUER })).unwrap(),
            "the discovery document names no jwks_uri",
        ),
        (
            document(ISSUER, "https://evil.example/jwks"),
            "the discovery document's jwks_uri is not https on the issuer's own origin",
        ),
        (
            document(ISSUER, "http://token.actions.githubusercontent.com/jwks"),
            "the discovery document's jwks_uri is not https on the issuer's own origin",
        ),
    ] {
        assert_eq!(jwks_uri(&body, ISSUER), Err(Untaken::Keys(why.to_owned())));
    }
    assert_eq!(
        discovery_of("https://ghes.example/_services/token/"),
        "https://ghes.example/_services/token/.well-known/openid-configuration"
    );
    assert!(same_origin(
        "https://ghes.example:443/a",
        "https://ghes.example/b"
    ));
    assert!(!same_origin(
        "https://ghes.example:8443/a",
        "https://ghes.example/b"
    ));
    assert!(!same_origin(
        "https://user@ghes.example/a",
        "https://ghes.example/b"
    ));
    assert!(!same_origin(
        "https://u:p@ghes.example/a",
        "https://ghes.example/b"
    ));
    assert!(!same_origin("not a url", "https://ghes.example/b"));
    assert!(!same_origin("https://ghes.example/a", "not a url"));
}

#[test]
fn every_refusal_reads_as_a_sentence_that_never_quotes_the_token() {
    for (untaken, said) in [
        (Untaken::Unreadable, "the ID token cannot be read"),
        (
            Untaken::Algorithm,
            "the ID token is signed with an algorithm its issuer's entry does not allow",
        ),
        (
            Untaken::Issuer,
            "no app takes ID tokens of the token's issuer",
        ),
        (
            Untaken::Audience,
            "the ID token is not meant for this herald",
        ),
        (Untaken::Time(Untaken::EXPIRED), "the ID token has expired"),
        (
            Untaken::Key,
            "the ID token's issuer publishes no key it names",
        ),
        (Untaken::Signature, "the ID token's signature does not hold"),
        (
            Untaken::Claim("ref".to_owned()),
            "the ID token's claim ref does not match its app's entry",
        ),
        (
            Untaken::Keys("down".to_owned()),
            "the token's issuer's keys could not be read: down",
        ),
        (
            Untaken::Used,
            "the ID token was taken before, and each is taken once",
        ),
        (
            Untaken::Unkept("down".to_owned()),
            "whether the ID token was taken before could not be kept: down",
        ),
    ] {
        assert_eq!(untaken.to_string(), said);
        assert_eq!(
            untaken.unavailable(),
            matches!(untaken, Untaken::Keys(_) | Untaken::Unkept(_)),
            "{untaken:?}"
        );
    }
    assert!(Untaken::Keys(String::new()).unavailable());
    assert!(Untaken::Unkept(String::new()).unavailable());
    assert!(!Untaken::Used.unavailable());
}

#[test]
fn only_an_algorithm_herald_checks_can_be_written() {
    assert_eq!(
        serde_json::from_value::<TokenAlgorithm>(json!("RS256")).unwrap(),
        TokenAlgorithm::Rs256
    );
    assert_eq!(
        serde_json::to_value(TokenAlgorithm::Rs256).unwrap(),
        json!("RS256")
    );
    for refused in ["none", "HS256", "rs256", "ES256"] {
        assert!(
            serde_json::from_value::<TokenAlgorithm>(json!(refused)).is_err(),
            "{refused}"
        );
    }
}

#[test]
fn a_kept_key_set_reads_back_its_keys_and_when_it_was_read_and_tried() {
    let body = serde_json::to_vec(&json!({ "keys": [
        { "kty": "RSA", "kid": "k1", "n": URL_SAFE_NO_PAD.encode([1, 2, 3]), "e": "AQAB" },
    ] }))
    .unwrap();
    let kept = KeptKeys::new(&body, now());
    assert_eq!((kept.fetched, kept.tried), (Some(now()), now()));
    let token = kept.kept("oidc/issuer".to_owned(), now());
    assert_eq!(token.name, "oidc/issuer");
    assert_eq!(token.expires, now());
    let read = KeptKeys::read(&token).unwrap();
    assert_eq!(read, kept);
    assert_eq!(read.keys().key(Some("k1")).unwrap().modulus, [1, 2, 3]);
    let later = Timestamp::from_second(NOW + 60).unwrap();
    let retried = kept.clone().tried_at(later);
    assert_eq!((retried.fetched, retried.tried), (Some(now()), later));
    assert_eq!(retried.keys(), kept.keys());
    let none = KeptKeys::untried(now());
    assert_eq!((none.fetched, none.keys()), (None, KeySet::default()));
    let written = String::from_utf8(
        none.kept("oidc/issuer".to_owned(), now())
            .sealed_bytes()
            .unwrap(),
    )
    .unwrap();
    assert!(!written.contains("fetched"), "{written}");
    assert_eq!(KeptKeys::new(b"not json", now()).keys(), KeySet::default());
    let garbled = KeptToken {
        sealed: "not base64!".to_owned(),
        ..token.clone()
    };
    assert_eq!(KeptKeys::read(&garbled), None);
    let shapeless = KeptToken::new("oidc/issuer".to_owned(), b"[]", now());
    assert_eq!(KeptKeys::read(&shapeless), None);
}

#[test]
fn a_token_is_meant_for_each_audience_its_aud_names_and_no_other() {
    let mut several = claims();
    several["aud"] = json!([AUDIENCE, "https://other.example"]);
    for (aud, audience, meant) in [
        (json!(AUDIENCE), AUDIENCE, true),
        (json!(AUDIENCE), "https://other.example", false),
        (several["aud"].clone(), "https://other.example", true),
        (several["aud"].clone(), "https://third.example", false),
        (json!(7), "7", false),
    ] {
        let mut named = claims();
        named["aud"] = aud;
        assert_eq!(read(&named).meant_for(audience), meant, "{audience}");
    }
}

#[test]
fn a_ci_is_asked_for_a_token_of_an_audience_and_answers_it_under_value() {
    assert_eq!(
        IdToken::asking("https://ci.example/idtoken?api-version=2.0", AUDIENCE),
        "&audience=https%3A%2F%2Fherald.example"
    );
    assert_eq!(
        IdToken::asking("https://ci.example/idtoken", "a b&c"),
        "?audience=a+b%26c"
    );
    assert_eq!(
        IdToken::answered(br#"{"count": 1, "value": "t.o.k"}"#).as_deref(),
        Some("t.o.k")
    );
    for unanswered in [
        &br#"{"value": 7}"#[..],
        br#"{"token": "t.o.k"}"#,
        br#"["t.o.k"]"#,
        b"t.o.k",
    ] {
        assert_eq!(IdToken::answered(unanswered), None);
    }
}

#[test]
fn a_token_is_within_its_times_by_its_expiry_and_any_not_before_alone() {
    let skew = SignedDuration::from_mins(5);
    let issued = |claims: Value| read(&claims).within(now(), skew);
    let unissued = json!({ "exp": NOW + 60, "nbf": NOW - 60 });
    assert_eq!(issued(unissued), Ok(NOW + 60));
    assert_eq!(issued(json!({ "exp": NOW + 1 })), Ok(NOW + 1));
    assert_eq!(issued(json!({ "exp": NOW - 299 })), Ok(NOW - 299));
    assert_eq!(issued(json!({ "exp": NOW - 300 })), Err(Untaken::EXPIRED));
    assert_eq!(
        issued(json!({ "exp": NOW + 600, "nbf": NOW + 300 })),
        Ok(NOW + 600)
    );
    assert_eq!(
        issued(json!({ "exp": NOW + 600, "nbf": NOW + 301 })),
        Err(Untaken::EARLY)
    );
    assert_eq!(issued(json!({ "nbf": NOW })), Err(Untaken::NO_EXPIRY));
    for unwhole in [
        json!({ "exp": "soon" }),
        json!({ "exp": NOW + 60, "nbf": 1.5 }),
    ] {
        assert_eq!(issued(unwhole), Err(Untaken::UNWHOLE));
    }
}

fn endorsed_set() -> Value {
    let n = URL_SAFE_NO_PAD.encode([1, 2, 3]);
    let e = URL_SAFE_NO_PAD.encode([1, 0, 1]);
    json!({ "keys": [
        { "kty": "RSA", "use": "sig", "kid": "teams", "n": n, "e": e, "endorsements": ["skype", "msteams"] },
        { "kty": "RSA", "use": "sig", "kid": "speech", "n": n, "e": e, "endorsements": ["telephony"] },
        { "kty": "RSA", "use": "sig", "kid": "plain", "n": n, "e": e },
        { "kty": "RSA", "use": "sig", "n": n, "e": e, "endorsements": [7, "web"] },
    ] })
}

#[test]
fn a_key_set_keeps_what_each_key_is_endorsed_for_and_narrows_to_one_endorsement() {
    let set = KeySet::read(&serde_json::to_vec(&endorsed_set()).unwrap()).unwrap();
    assert_eq!(
        set.key(Some("teams")).unwrap().endorsements,
        ["skype", "msteams"]
    );
    assert_eq!(set.key(Some("plain")).unwrap().endorsements, [""; 0]);
    assert!(!set.unendorsed(Some("speech")));
    let teams = set.endorsed("msteams");
    let kept = teams.key(Some("teams")).unwrap();
    assert_eq!(
        (&kept.modulus, &kept.exponent, kept.endorsements.is_empty()),
        (&vec![1, 2, 3], &vec![1, 0, 1], true)
    );
    for other in ["speech", "plain"] {
        assert_eq!(teams.key(Some(other)), None, "{other}");
        assert!(teams.unendorsed(Some(other)), "{other}");
    }
    assert!(!teams.unendorsed(Some("teams")));
    assert!(!teams.unendorsed(None));
    assert!(!teams.unendorsed(Some("absent")));
    let again = KeySet::read(&teams.written()).unwrap();
    assert_eq!(again, teams);
    let whole = KeySet::read(&set.written()).unwrap();
    assert_eq!(whole, set);
    assert!(set.endorsed("outlook").key(None).is_none());
    assert!(set.endorsed("outlook").is_empty());
    let unnamed =
        KeySet::read(br#"{"keys":[{"kty":"RSA","n":"AQ","e":"AQAB","endorsements":["web"]}]}"#)
            .unwrap()
            .endorsed("web");
    assert_eq!(KeySet::read(&unnamed.written()).unwrap(), unnamed);
    assert_eq!(unnamed.key(None).unwrap().id, None);
}

#[test]
fn a_key_set_listing_more_keys_than_herald_reads_is_refused() {
    let key = json!({ "kty": "RSA", "kid": "k", "n": "AQ", "e": "AQAB" });
    let listed =
        |count: usize| serde_json::to_vec(&json!({ "keys": vec![key.clone(); count] })).unwrap();
    assert!(KeySet::read(&listed(KeySet::MOST)).is_ok());
    assert_eq!(
        KeySet::read(&listed(KeySet::MOST + 1)),
        Err(Untaken::Keys(
            "the key set lists more than 1024 keys".to_owned()
        ))
    );
}

#[test]
fn a_metadata_document_names_keys_on_its_own_origin_and_the_algorithms_it_signs_with() {
    let metadata = "https://login.botframework.com/v1/.well-known/openidconfiguration";
    let issuer = "https://api.botframework.com";
    let document = |jwks: &str, algorithms: Value| {
        serde_json::to_vec(&json!({
            "issuer": issuer,
            "jwks_uri": jwks,
            "id_token_signing_alg_values_supported": algorithms,
        }))
        .unwrap()
    };
    let keys = "https://login.botframework.com/v1/.well-known/keys";
    let own = document(keys, json!(["RS256"]));
    assert_eq!(jwks_uri_on(&own, issuer, metadata), Ok(keys.to_owned()));
    assert!(matches!(jwks_uri(&own, issuer), Err(Untaken::Keys(_))));
    assert!(matches!(
        jwks_uri_on(
            &document("https://api.botframework.com/keys", json!([])),
            issuer,
            metadata
        ),
        Err(Untaken::Keys(_))
    ));
    assert!(matches!(
        jwks_uri_on(&own, "https://api.botframework.us", metadata),
        Err(Untaken::Keys(_))
    ));
    assert!(signs_with(&own, TokenAlgorithm::Rs256));
    for unsigned in [
        document(keys, json!(["RS512"])),
        document(keys, json!("RS256")),
        document(keys, json!([256])),
        b"{}".to_vec(),
        b"no".to_vec(),
    ] {
        assert!(!signs_with(&unsigned, TokenAlgorithm::Rs256));
    }
}
