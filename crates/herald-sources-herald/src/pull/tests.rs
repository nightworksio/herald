use std::num::NonZeroUsize;

use herald_core::{Cursor, Identity, KindSpecs, Size};
use herald_kinds::{Origin as Trusted, Pull, Pulling, Trust, Unpulled};
use herald_peers::answer::{CONTENT_TYPE, ETAG, demanded, demanded_unchanged};
use herald_peers::digest::{CONTENT_DIGEST, content_digest};
use herald_peers::keys::method;
use herald_peers::message::{Component, Covered, Message};
use herald_ports::Response;
use herald_testing::{FakeVerifier, ScriptedHttp, fake_ed25519};
use jiff::Timestamp;
use serde_json::{Value, json};

use super::*;
use crate::HeraldKind;

mod proofs;
use crate::options::HeraldFile;
use herald_kinds::SourceKind as _;

const DOMAIN: &str = "peer.example";
const URL: &str = "https://peer.example";
const PEER: &str = "https://peer.example/.well-known/herald";
const ORIGIN: &str = "https://origin.example/.well-known/herald";
const FEED: &str = "https://peer.example/herald/v1/feeds/releases";
const KEY: PublicKey = [7; 32];
const ORIGIN_KEY: PublicKey = [9; 32];
const STRANGER: PublicKey = [3; 32];
const NOW: i64 = 1_800_000_000;

fn trust() -> Trust {
    Trust {
        url: URL.to_owned(),
        domain: DOMAIN.to_owned(),
        keys: vec![KEY],
        origins: vec![Trusted {
            name: "origin".to_owned(),
            url: "https://origin.example".to_owned(),
            domain: "origin.example".to_owned(),
            keys: vec![ORIGIN_KEY],
        }],
        most: 2 * Size::MIB,
        hops: 2,
        fields: 8192,
        skew: 300,
        longest: 1200,
    }
}

fn source(file: HeraldFile) -> Pulled {
    let options = HeraldKind::checked(HeraldFile {
        peer: Some("upstream".to_owned()),
        feed: Some("releases".to_owned()),
        ..file
    })
    .unwrap();
    Pulled::new(&options.served_at(URL))
}

fn proved(envelope: &Value, domain: &str, key: &PublicKey) -> Value {
    let unproved = proof::unproved(
        envelope,
        &method(domain, key),
        Timestamp::from_second(NOW).unwrap(),
    )
    .unwrap();
    let signature = fake_ed25519(key, unproved.message());
    unproved.proved(&signature)
}

fn bare(identity: &str) -> Value {
    json!({
        "identity": identity,
        "kind": "release",
        "project": "gate",
        "title": "gate v1",
        "body": "the gate",
        "links": { "release": "https://forge.example/gate/releases/v1" },
        "attributes": { "version": "v1", "colour": "blue" },
        "occurred": "2026-11-17T10:00:00Z",
        "visibility": "public",
        "type": "herald:Event",
        "path": [PEER],
        "origin": { "forge": "github", "host": "github.com", "repository": "acme/gate" },
    })
}

fn envelope(identity: &str) -> Value {
    proved(&bare(identity), DOMAIN, &KEY)
}

fn signed(url: &str, body: Vec<u8>, keyid: &PublicKey, by: &PublicKey) -> Response {
    let mut components = demanded().to_vec();
    components.push(Component::Field(ETAG.to_owned()));
    answered((url, 200, body), components, keyid, by)
}

/// `body`, answering a GET of `url` with `status` and the entity tag
/// `"9.0.20"`, its signature over `components` naming `keyid` and made by
/// `by`.
fn answered(
    (url, status, body): (&str, u16, Vec<u8>),
    components: Vec<Component>,
    keyid: &PublicKey,
    by: &PublicKey,
) -> Response {
    let mut headers = vec![
        (CONTENT_TYPE.to_owned(), "application/json".to_owned()),
        (CONTENT_DIGEST.to_owned(), content_digest(&body)),
        (ETAG.to_owned(), "\"9.0.20\"".to_owned()),
    ];
    let covered = Covered {
        components,
        created: NOW,
        expires: NOW + 300,
        nonce: None,
        keyid: method(DOMAIN, keyid),
    };
    let message = Message {
        method: "GET",
        url,
        status: Some(status),
        fields: &headers,
    };
    let base = covered.base(&message).unwrap();
    headers.extend(covered.fields(&fake_ed25519(by, &base)));
    Response {
        status,
        headers,
        body,
    }
}

fn listed(url: &str, items: &[(u64, Value)], more: bool) -> Vec<u8> {
    let items: Vec<Value> = items
        .iter()
        .map(|(sequence, envelope)| json!({ "sequence": sequence, "envelope": envelope }))
        .collect();
    let mut page = json!({ "id": url, "feed": FEED, "items": items });
    if more {
        page["next"] = json!(format!("{FEED}?before=1"));
    }
    page.to_string().into_bytes()
}

fn page(url: &str, items: &[(u64, Value)], more: bool) -> Response {
    signed(url, listed(url, items, more), &KEY, &KEY)
}

fn pulled(
    source: &Pulled,
    answers: Vec<Response>,
    cursor: Option<&Cursor>,
) -> (Result<Option<Polled>, Unpulled>, ScriptedHttp) {
    pulled_trusting(source, answers, cursor, &trust())
}

fn pulled_trusting(
    source: &Pulled,
    answers: Vec<Response>,
    cursor: Option<&Cursor>,
    trust: &Trust,
) -> (Result<Option<Polled>, Unpulled>, ScriptedHttp) {
    let http = ScriptedHttp::new(answers);
    let specs = KindSpecs::default();
    let pulling = Pulling {
        http: &http,
        verifier: &FakeVerifier,
        specs: &specs,
        now: Timestamp::from_second(NOW).unwrap(),
    };
    assert!(format!("{pulling:?}").starts_with("Pulling { now: "));
    let read = pollster::block_on(source.pulled(pulling, trust, cursor));
    (read, http)
}

fn identities(polled: &Polled) -> Vec<&str> {
    polled
        .events
        .iter()
        .map(|event| event.identity().as_str())
        .collect()
}

fn passed(polled: &Polled) -> Vec<&str> {
    polled.passed.iter().map(Identity::as_str).collect()
}

fn read(source: &Pulled, items: &[(u64, Value)]) -> Polled {
    pulled(source, vec![page(FEED, items, false)], None)
        .0
        .unwrap()
        .unwrap()
}

#[test]
fn a_first_pull_reads_one_page_and_takes_each_event_under_the_peer() {
    let source = source(HeraldFile::default());
    let (read, http) = pulled(
        &source,
        vec![page(
            FEED,
            &[(9, envelope("a/1")), (8, envelope("a/2"))],
            true,
        )],
        None,
    );
    let read = read.unwrap().unwrap();
    assert_eq!(
        identities(&read),
        ["herald/upstream/a/1", "herald/upstream/a/2"]
    );
    assert_eq!(read.sequence, Some(9));
    assert_eq!(read.etag.as_deref(), Some("\"9.0.20\""));
    assert_eq!(read.watched, 2);
    assert_eq!(http.asked(), [(FEED.to_owned(), None)]);
    let event = &read.events[0];
    let origin = event.origin().unwrap();
    assert_eq!(
        (
            origin.forge.as_str(),
            origin.host.as_str(),
            origin.repository.as_str()
        ),
        ("herald", DOMAIN, "upstream")
    );
    assert_eq!(
        (origin.owner.as_str(), origin.id.as_str()),
        ("", "herald/upstream")
    );
    assert_eq!(origin.visibility, Visibility::Public);
    let text = |name: &str| match event.attribute(name) {
        Some(Attribute::Text(text)) => text.clone(),
        other => panic!("{name}: {other:?}"),
    };
    assert_eq!(text(CLAIMED_FORGE), "github");
    assert_eq!(text(CLAIMED_HOST), "github.com");
    assert_eq!(text(CLAIMED_REPOSITORY), "acme/gate");
    assert_eq!(text("version"), "v1");
    assert_eq!(event.attribute("colour"), None);
    let request = &http.requests()[0];
    assert_eq!(request.most, 2 * Size::MIB);
}

#[test]
fn a_later_pull_reads_down_to_its_sequence_asking_the_first_page_for_a_change() {
    let source = source(HeraldFile::default());
    let cursor = Cursor {
        sequence: Some(5),
        etag: Some("\"5.0.20\"".to_owned()),
        ..Cursor::default()
    };
    let before = format!("{FEED}?before=8");
    let (read, http) = pulled(
        &source,
        vec![
            page(FEED, &[(9, envelope("a/9")), (8, envelope("a/8"))], true),
            page(&before, &[(7, envelope("a/7")), (5, envelope("a/5"))], true),
        ],
        Some(&cursor),
    );
    let read = read.unwrap().unwrap();
    assert_eq!(
        identities(&read),
        [
            "herald/upstream/a/9",
            "herald/upstream/a/8",
            "herald/upstream/a/7"
        ]
    );
    assert_eq!(read.sequence, Some(9));
    assert_eq!(
        http.asked(),
        [
            (FEED.to_owned(), Some("\"5.0.20\"".to_owned())),
            (before, None)
        ]
    );
    let quiet = pulled(
        &source,
        vec![page(FEED, &[(5, envelope("a/5"))], true)],
        Some(&cursor),
    )
    .0
    .unwrap()
    .unwrap();
    assert_eq!(quiet.events, []);
    assert_eq!(quiet.sequence, None);
}

#[test]
fn an_unchanged_feed_reads_nothing_once_its_answer_is_signed_over_the_tag_held() {
    let source = source(HeraldFile::default());
    let held = |etag: &str| Cursor {
        etag: Some(etag.to_owned()),
        ..Cursor::default()
    };
    let unchanged = |keyid: &PublicKey, by: &PublicKey| {
        answered(
            (FEED, 304, Vec::new()),
            demanded_unchanged().to_vec(),
            keyid,
            by,
        )
    };
    let cursor = held("\"9.0.20\"");
    let read = |answer: Response, cursor: &Cursor| pulled(&source, vec![answer], Some(cursor)).0;
    assert_eq!(read(unchanged(&KEY, &KEY), &cursor), Ok(None));
    assert_eq!(
        read(unchanged(&KEY, &KEY), &held("\"8.0.20\"")),
        Err(Unpulled::Unreadable(
            "the answer says nothing changed of another entity tag than the one asked for"
                .to_owned()
        ))
    );
    assert_eq!(
        read(unchanged(&STRANGER, &STRANGER), &cursor),
        Err(Unpulled::Stranger(method(DOMAIN, &STRANGER)))
    );
    assert_eq!(
        read(unchanged(&KEY, &STRANGER), &cursor),
        Err(Unpulled::Unreadable(
            "the feed's answer's signature does not hold".to_owned()
        ))
    );
    let unsigned = Response {
        status: 304,
        headers: vec![(ETAG.to_owned(), "\"9.0.20\"".to_owned())],
        body: Vec::new(),
    };
    assert_eq!(
        read(unsigned, &cursor),
        Err(Unpulled::Unreadable(
            "the message carries no signature tagged herald".to_owned()
        ))
    );
    let mut untagged = demanded_unchanged().to_vec();
    untagged.remove(1);
    assert_eq!(
        read(
            answered((FEED, 304, Vec::new()), untagged, &KEY, &KEY),
            &cursor
        ),
        Err(Unpulled::Unreadable(
            "the signature does not cover etag".to_owned()
        ))
    );
    assert_eq!(
        pulled(&source, vec![unchanged(&KEY, &KEY)], None).0,
        Err(Unpulled::Unreadable("the feed was answered 304".to_owned()))
    );
}

#[test]
fn an_entity_tag_the_answer_s_signature_does_not_cover_is_not_kept() {
    let source = source(HeraldFile::default());
    let body = listed(FEED, &[(9, envelope("a/9"))], false);
    let untagged = answered((FEED, 200, body), demanded().to_vec(), &KEY, &KEY);
    let read = pulled(&source, vec![untagged], None).0.unwrap().unwrap();
    assert_eq!(read.etag, None);
    assert_eq!(identities(&read), ["herald/upstream/a/9"]);
}

#[test]
fn a_pull_reads_at_most_its_pages_and_with_no_cursor_enough_for_its_backfill() {
    let one = source(HeraldFile {
        pages: NonZeroUsize::new(1),
        ..HeraldFile::default()
    });
    let cursor = Cursor {
        sequence: Some(1),
        ..Cursor::default()
    };
    let (_, http) = pulled(
        &one,
        vec![page(FEED, &[(9, envelope("a/9"))], true)],
        Some(&cursor),
    );
    assert_eq!(http.asked().len(), 1);
    let filling = source(HeraldFile {
        backfill: 3,
        ..HeraldFile::default()
    });
    let second = format!("{FEED}?before=8");
    let third = format!("{FEED}?before=6");
    let (read, http) = pulled(
        &filling,
        vec![
            page(FEED, &[(9, envelope("a/9")), (8, envelope("a/8"))], true),
            page(&second, &[(7, envelope("a/7")), (6, envelope("a/6"))], true),
            page(&third, &[(5, envelope("a/5"))], true),
        ],
        None,
    );
    assert_eq!(read.unwrap().unwrap().events.len(), 4);
    assert_eq!(http.asked().len(), 2);
    let exact = source(HeraldFile {
        backfill: 2,
        ..HeraldFile::default()
    });
    let (_, http) = pulled(
        &exact,
        vec![page(
            FEED,
            &[(9, envelope("a/9")), (8, envelope("a/8"))],
            true,
        )],
        None,
    );
    assert_eq!(http.asked().len(), 1);
}

#[test]
fn an_answer_is_read_only_once_a_trusted_key_signed_it() {
    let source = source(HeraldFile::default());
    let body = listed(FEED, &[(9, envelope("a/9"))], false);
    assert_eq!(
        pulled(
            &source,
            vec![signed(FEED, body.clone(), &STRANGER, &STRANGER)],
            None
        )
        .0,
        Err(Unpulled::Stranger(method(DOMAIN, &STRANGER)))
    );
    assert_eq!(
        pulled(
            &source,
            vec![signed(FEED, body.clone(), &KEY, &STRANGER)],
            None
        )
        .0,
        Err(Unpulled::Unreadable(
            "the feed's answer's signature does not hold".to_owned()
        ))
    );
    let unsigned = Response {
        status: 200,
        headers: Vec::new(),
        body,
    };
    assert!(matches!(
        pulled(&source, vec![unsigned], None).0,
        Err(Unpulled::Unreadable(_))
    ));
    let failed = Response {
        status: 500,
        headers: Vec::new(),
        body: Vec::new(),
    };
    assert_eq!(
        pulled(&source, vec![failed], None).0,
        Err(Unpulled::Unreadable("the feed was answered 500".to_owned()))
    );
    assert_eq!(
        pulled(&source, Vec::new(), None).0,
        Err(Unpulled::Unreachable(
            "the host did not answer in time".to_owned()
        ))
    );
    let elsewhere = signed(
        FEED,
        listed("https://peer.example/x", &[], false),
        &KEY,
        &KEY,
    );
    assert_eq!(
        pulled(&source, vec![elsewhere], None).0,
        Err(Unpulled::Unreadable(
            "the feed answered with the page https://peer.example/x, not the one asked".to_owned()
        ))
    );
}

#[test]
fn an_envelope_is_taken_only_once_its_proof_holds_for_the_instance_that_made_it() {
    let source = source(HeraldFile::default());
    let refused = |envelope: Value| {
        pulled(&source, vec![page(FEED, &[(9, envelope)], false)], None)
            .0
            .unwrap_err()
    };
    assert_eq!(
        refused(json!({ "type": "herald:Event" })),
        Unpulled::Unreadable(
            "the feed's page cannot be read: it names no path of instances".to_owned()
        )
    );
    assert_eq!(
        refused(bare("a/1")),
        Unpulled::Unreadable("the object carries no proof".to_owned())
    );
    assert_eq!(
        refused(proved(&bare("a/1"), DOMAIN, &STRANGER)),
        Unpulled::Unreadable(format!(
            "an envelope is proved by {}, a key not trusted for {DOMAIN}",
            method(DOMAIN, &STRANGER)
        ))
    );
    let mut forged = envelope("a/1");
    forged["title"] = json!("gate v2");
    assert_eq!(
        refused(forged),
        Unpulled::Unreadable("an envelope's proof does not hold".to_owned())
    );
    let tombstone = proved(
        &json!({ "type": "herald:Tombstone", "identity": "a/1", "deleted": "2026-11-17T10:00:00Z", "path": [PEER] }),
        DOMAIN,
        &KEY,
    );
    let unknown = read(&source, &[(9, tombstone.clone())]);
    assert!(unknown.events.is_empty() && unknown.passed.is_empty());
    assert_eq!(unknown.withdrawn, Vec::<Identity>::new());
    assert_eq!(unknown.sequence, Some(9));
    assert!(unknown.relisted);
    let took = Cursor {
        seen: vec![Identity::new("herald/upstream/a/1")],
        ..Cursor::default()
    };
    let withdrawn = pulled(
        &source,
        vec![page(FEED, &[(9, tombstone)], false)],
        Some(&took),
    )
    .0
    .unwrap()
    .unwrap();
    assert_eq!(withdrawn.withdrawn, [Identity::new("herald/upstream/a/1")]);
}

#[test]
fn a_relayed_envelope_is_taken_only_by_hop_from_a_peer_through_few_enough_instances() {
    let relayed = |path: Value, domain: &str, key: &PublicKey| {
        let mut envelope = bare("a/1");
        envelope["path"] = path;
        proved(&envelope, domain, key)
    };
    let origin = source(HeraldFile::default());
    let read = read(
        &origin,
        &[(
            9,
            relayed(json!([ORIGIN, PEER]), "origin.example", &ORIGIN_KEY),
        )],
    );
    assert_eq!(passed(&read), ["herald/upstream/a/1"]);
    let hop = source(HeraldFile {
        verify: Verify::Hop,
        ..HeraldFile::default()
    });
    let taken = super::tests::read(
        &hop,
        &[(
            9,
            relayed(json!([ORIGIN, PEER]), "origin.example", &ORIGIN_KEY),
        )],
    );
    assert_eq!(identities(&taken), ["herald/origin/a/1"]);
    let origin_of = taken.events[0].origin().unwrap();
    assert_eq!(
        (
            origin_of.forge.as_str(),
            origin_of.host.as_str(),
            origin_of.repository.as_str()
        ),
        ("herald", "origin.example", "origin")
    );
    assert_eq!(
        pulled(
            &hop,
            vec![page(
                FEED,
                &[(9, relayed(json!([ORIGIN, PEER]), "origin.example", &KEY))],
                false
            )],
            None
        )
        .0,
        Err(Unpulled::Unreadable(format!(
            "an envelope is proved by {}, a key not trusted for origin.example",
            method("origin.example", &KEY)
        )))
    );
    for path in [
        json!([ORIGIN, "https://between.example/.well-known/herald", PEER]),
        json!([ORIGIN, ORIGIN]),
        json!([PEER, PEER]),
        json!([ORIGIN, "https://between.example/.well-known/herald"]),
        json!(["https://unknown.example/.well-known/herald", PEER]),
    ] {
        let skipped = super::tests::read(
            &hop,
            &[(9, relayed(path.clone(), "origin.example", &ORIGIN_KEY))],
        );
        assert_eq!(passed(&skipped), ["herald/upstream/a/1"], "{path}");
    }
}

#[test]
fn an_event_of_no_declared_kind_or_out_of_its_kind_s_shape_is_passed() {
    let source = source(HeraldFile::default());
    let changed = |name: &str, value: Value| {
        let mut envelope = bare("a/1");
        envelope[name] = value;
        proved(&envelope, DOMAIN, &KEY)
    };
    for envelope in [
        changed("kind", json!("tag")),
        changed("kind", json!("Not A Kind")),
        changed("attributes", json!({ "version": 1 })),
        changed("attributes", json!({ "version": { "major": 1 } })),
        changed("title", json!(" ")),
    ] {
        let read = read(&source, &[(9, envelope.clone())]);
        assert_eq!(passed(&read), ["herald/upstream/a/1"], "{envelope}");
        assert_eq!(read.events, []);
    }
}

#[test]
fn a_granted_identity_keeps_itself_and_the_origin_it_names() {
    let source = source(HeraldFile {
        namespaces: vec!["github.com/acme".to_owned()],
        ..HeraldFile::default()
    });
    let read = read(
        &source,
        &[
            (9, envelope("github.com/acme/gate/v1")),
            (8, envelope("github.com/acmes/v1")),
        ],
    );
    assert_eq!(
        identities(&read),
        [
            "github.com/acme/gate/v1",
            "herald/upstream/github.com/acmes/v1"
        ]
    );
    let origin = read.events[0].origin().unwrap();
    assert_eq!(
        (
            origin.forge.as_str(),
            origin.host.as_str(),
            origin.owner.as_str(),
            origin.repository.as_str(),
            origin.id.as_str()
        ),
        (
            "herald",
            "github.com",
            "acme",
            "acme/gate",
            "herald/upstream/github.com/acme/gate"
        )
    );
    assert_eq!(
        read.events[0].attribute(CLAIMED_FORGE),
        Some(&Attribute::Text("github".to_owned()))
    );
    assert_eq!(read.events[1].origin().unwrap().forge, "herald");
    let mut unnamed = bare("github.com/acme/gate/v2");
    unnamed["origin"] =
        json!({ "forge": "Not A Forge", "host": "github.com", "repository": "acme/ga te" });
    let mut bare_origin = bare("github.com/acme/gate/v3");
    bare_origin.as_object_mut().unwrap().remove("origin");
    let fallen = super::tests::read(
        &source,
        &[
            (9, proved(&unnamed, DOMAIN, &KEY)),
            (8, proved(&bare_origin, DOMAIN, &KEY)),
        ],
    );
    let origin = fallen.events[0].origin().unwrap();
    assert_eq!(
        (origin.forge.as_str(), origin.repository.as_str()),
        ("herald", "upstream")
    );
    assert_eq!(
        fallen.events[0].attribute(CLAIMED_FORGE),
        Some(&Attribute::Text("Not A Forge".to_owned()))
    );
    assert_eq!(fallen.events[1].origin().unwrap().forge, "herald");
    assert_eq!(fallen.events[1].attribute(CLAIMED_HOST), None);
}

#[test]
fn a_grant_holds_only_for_identities_the_peer_made_itself() {
    let hop = source(HeraldFile {
        verify: Verify::Hop,
        namespaces: vec!["github.com/acme".to_owned()],
        ..HeraldFile::default()
    });
    let mut relayed = bare("github.com/acme/gate/v1");
    relayed["path"] = json!([ORIGIN, PEER]);
    let read = read(
        &hop,
        &[
            (9, proved(&relayed, "origin.example", &ORIGIN_KEY)),
            (8, envelope("github.com/acme/gate/v1")),
        ],
    );
    assert_eq!(
        identities(&read),
        [
            "herald/origin/github.com/acme/gate/v1",
            "github.com/acme/gate/v1"
        ]
    );
    let relayed_origin = read.events[0].origin().unwrap();
    assert_eq!(
        (relayed_origin.forge.as_str(), relayed_origin.host.as_str()),
        ("herald", "origin.example")
    );
    assert_eq!(
        read.events[0].attribute(CLAIMED_REPOSITORY),
        Some(&Attribute::Text("acme/gate".to_owned()))
    );
    let own = read.events[1].origin().unwrap();
    assert_eq!(
        (own.forge.as_str(), own.repository.as_str()),
        ("herald", "acme/gate")
    );
}

#[test]
fn a_peer_s_claim_to_our_repository_matches_our_origin_globs_only_where_granted() {
    let ours = herald_core::Match {
        repositories: vec![herald_core::Glob::new("acme/*").unwrap()],
        ..herald_core::Match::default()
    };
    let hosted = herald_core::Match {
        hosts: vec!["github.com".to_owned()],
        ..herald_core::Match::default()
    };
    let forged = herald_core::Match {
        forges: vec!["github".to_owned()],
        ..herald_core::Match::default()
    };
    let ungranted = read(
        &source(HeraldFile::default()),
        &[(9, envelope("github.com/acme/gate/v1"))],
    );
    let granted = read(
        &source(HeraldFile {
            namespaces: vec!["github.com/acme".to_owned()],
            ..HeraldFile::default()
        }),
        &[(9, envelope("github.com/acme/gate/v1"))],
    );
    for matching in [&ours, &hosted] {
        assert!(!matching.takes(&ungranted.events[0]), "{matching:?}");
        assert!(matching.takes(&granted.events[0]), "{matching:?}");
    }
    assert!(!forged.takes(&ungranted.events[0]));
    assert!(!forged.takes(&granted.events[0]));
}

#[test]
fn a_relayed_path_naming_an_instance_twice_is_passed_however_many_hops_it_may_take() {
    let hop = source(HeraldFile {
        verify: Verify::Hop,
        ..HeraldFile::default()
    });
    let far = Trust { hops: 3, ..trust() };
    let relayed = |path: Value| {
        let mut envelope = bare("a/1");
        envelope["path"] = path;
        proved(&envelope, "origin.example", &ORIGIN_KEY)
    };
    let read = |path: Value| {
        pulled_trusting(
            &hop,
            vec![page(FEED, &[(9, relayed(path))], false)],
            None,
            &far,
        )
        .0
        .unwrap()
        .unwrap()
    };
    let looped = read(json!([ORIGIN, ORIGIN, PEER]));
    assert_eq!(passed(&looped), ["herald/upstream/a/1"]);
    let through = read(json!([
        ORIGIN,
        "https://between.example/.well-known/herald",
        PEER
    ]));
    assert_eq!(identities(&through), ["herald/origin/a/1"]);
}

#[test]
fn a_granted_identity_keeps_only_a_claimed_origin_under_its_namespace() {
    let source = source(HeraldFile {
        namespaces: vec!["github.com/acme".to_owned()],
        ..HeraldFile::default()
    });
    let claiming = |identity: &str, repository: &str| {
        let mut envelope = bare(identity);
        envelope["origin"] =
            json!({ "forge": "github", "host": "github.com", "repository": repository });
        proved(&envelope, DOMAIN, &KEY)
    };
    let read = read(
        &source,
        &[
            (9, claiming("github.com/acme/gate/v1", "acme/gate")),
            (8, claiming("github.com/acme/gate/v2", "ours/secret")),
            (7, claiming("github.com/acme/gate/v3", "acme")),
            (6, claiming("github.com/acme/gate/v4", "acmes/gate")),
        ],
    );
    let origins: Vec<(&str, &str)> = read
        .events
        .iter()
        .map(|event| {
            let origin = event.origin().unwrap();
            (origin.forge.as_str(), origin.repository.as_str())
        })
        .collect();
    assert_eq!(
        origins,
        [
            ("herald", "acme/gate"),
            ("herald", "upstream"),
            ("herald", "acme"),
            ("herald", "upstream")
        ]
    );
    assert_eq!(
        read.events[1].attribute(CLAIMED_REPOSITORY),
        Some(&Attribute::Text("ours/secret".to_owned()))
    );
}

#[test]
fn a_page_with_nothing_older_ends_the_pull_short_of_the_cursor() {
    let source = source(HeraldFile::default());
    let cursor = Cursor {
        sequence: Some(1),
        ..Cursor::default()
    };
    let (read, http) = pulled(
        &source,
        vec![page(
            FEED,
            &[(9, envelope("a/9")), (8, envelope("a/8"))],
            false,
        )],
        Some(&cursor),
    );
    assert_eq!(read.unwrap().unwrap().events.len(), 2);
    assert_eq!(http.asked().len(), 1);
}
