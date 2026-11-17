//! herald's API as peers read it: each feed in pages, newest first, and
//! every answer signed by the instance's newest key over its body's digest
//! and the request it answers.

use herald_core::{DestinationName, Feed};
use herald_peers::digest::content_digest;
use herald_peers::message::{Component, Message, presented};
use herald_ports::Published as _;
use serde_json::{Value, json};

use super::*;

fn fed(keys: &str, extra: &str) -> Config {
    let text = format!(
        "{}\n[federation]\ndomain = \"herald.example\"\n{keys}\n[destinations.releases]\nkind = \"herald\"\npage = 2\n[[routes]]\nid = \"fed\"\nsources = [\"gate\"]\ndestination = \"releases\"\n{extra}",
        config_text(),
    );
    load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

fn world() -> World {
    World::with(
        fed("keys = [{ secret = \"HERALD_KEY_NEW\" }]", ""),
        Script::default(),
    )
}

fn filled(world: &World, envelopes: u64) {
    let feed = (1..=envelopes).fold(Feed::default(), |feed, at| {
        feed.entered(json!({ "identity": format!("github.com/1/v{at}") }), 10)
    });
    pollster::block_on(
        world
            .store
            .inner
            .put_feed(&DestinationName::new("releases"), &feed, None),
    )
    .unwrap();
}

fn header<'a>(answered: &'a Answered, name: &str) -> Option<&'a str> {
    answered
        .reply
        .headers
        .iter()
        .find(|(named, _)| named == name)
        .map(|(_, value)| value.as_str())
}

fn page(answered: &Answered) -> Value {
    let (status, kind, body) = said(answered);
    assert_eq!((status, kind), (200, "application/json"), "{body}");
    serde_json::from_str(&body).unwrap()
}

fn sequences(page: &Value) -> Vec<u64> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["sequence"].as_u64().unwrap())
        .collect()
}

const FEED: &str = "https://herald.example/herald/v1/feeds/releases";

fn asked<'a>(query: Option<&'a str>, headers: &'a [(&'a str, &'a str)]) -> Inbound<'a> {
    Inbound {
        query,
        headers,
        ..get("/herald/v1/feeds/releases")
    }
}

/// Whether `answered`, to `inbound`, carries a signature by the newest key
/// over its status, type, digest, tag where it has one, and the request.
fn signed_over(answered: &Answered, inbound: &Inbound<'_>, url: &str) -> Vec<Component> {
    let fields: Vec<(String, String)> =
        std::iter::once(("content-type".to_owned(), answered.reply.kind.to_owned()))
            .chain(answered.reply.headers.iter().cloned())
            .collect();
    let message = Message {
        method: inbound.method,
        url,
        status: Some(answered.reply.status),
        fields: &fields,
    };
    let shown = presented(&message, 8192).unwrap();
    assert_eq!(
        header(answered, "content-digest"),
        Some(content_digest(&answered.reply.body).as_str())
    );
    let key: [u8; 32] = NEW_KEY.as_bytes().try_into().unwrap();
    assert_eq!(
        shown.covered.keyid,
        herald_peers::keys::method("herald.example", &key)
    );
    assert_eq!(shown.covered.expires - shown.covered.created, 300);
    assert_eq!(shown.covered.nonce, None);
    assert_eq!(
        shown.signature,
        [
            b"Ed25519|".as_slice(),
            NEW_KEY.as_bytes(),
            b"|",
            &shown.base
        ]
        .concat()
    );
    shown.covered.components
}

fn request_parts() -> [Component; 2] {
    [
        Component::Request(Box::new(Component::Method)),
        Component::Request(Box::new(Component::TargetUri)),
    ]
}

#[test]
fn a_feed_is_listed_newest_first_in_pages_each_signed() {
    let world = world();
    filled(&world, 3);
    let inbound = asked(None, &[("Host", "herald.example")]);
    let answered = world.answer(&inbound);
    let first = page(&answered);
    assert_eq!(first["id"], FEED);
    assert_eq!(first["feed"], FEED);
    assert_eq!(sequences(&first), [3, 2]);
    assert_eq!(first["next"], format!("{FEED}?before=2"));
    assert_eq!(header(&answered, "etag"), Some("\"3.0.2\""));
    let field = |name: &str| Component::Field(name.to_owned());
    let mut components = vec![
        Component::Status,
        field("content-type"),
        field("content-digest"),
        field("etag"),
    ];
    components.extend(request_parts());
    assert_eq!(signed_over(&answered, &inbound, FEED), components);
    let next = asked(Some("x=1&before=2"), &[("Host", "herald.example")]);
    let answered = world.answer(&next);
    let last = page(&answered);
    assert_eq!(sequences(&last), [1]);
    assert_eq!(last["id"], format!("{FEED}?before=2"));
    assert_eq!(last.get("next"), None);
    assert_eq!(header(&answered, "etag"), Some("\"3.2.2\""));
    signed_over(&answered, &next, &format!("{FEED}?x=1&before=2"));
}

#[test]
fn a_reader_holding_the_page_s_tag_is_told_it_is_unchanged() {
    let world = world();
    filled(&world, 1);
    let inbound = asked(
        None,
        &[("Host", "herald.example"), ("If-None-Match", "\"1.0.2\"")],
    );
    let answered = world.answer(&inbound);
    assert_eq!(answered.reply.status, 304);
    assert_eq!(answered.reply.body, Vec::<u8>::new());
    assert_eq!(header(&answered, "etag"), Some("\"1.0.2\""));
    let covered = signed_over(&answered, &inbound, FEED);
    for demanded in herald_peers::answer::demanded_unchanged() {
        assert!(covered.contains(&demanded), "{demanded:?}");
    }
    let stale = asked(
        None,
        &[("Host", "herald.example"), ("If-None-Match", "\"0.0.2\"")],
    );
    assert_eq!(world.answer(&stale).reply.status, 200);
}

#[test]
fn an_empty_feed_lists_nothing() {
    let first = page(&world().answer(&get("/herald/v1/feeds/releases")));
    assert_eq!(first["items"], json!([]));
    assert_eq!(first.get("next"), None);
}

#[test]
fn a_page_no_feed_or_host_serves_is_missing_and_still_signed() {
    let world = world();
    for (path, query, host) in [
        (
            "/herald/v1/feeds/releases",
            Some("before=x"),
            "herald.example",
        ),
        ("/herald/v1/feeds/nowhere", None, "herald.example"),
        ("/herald/v1/feeds/%zz", None, "herald.example"),
        ("/herald/v1/feeds/discord", None, "herald.example"),
        ("/herald/v1/feeds/releases", None, "other.example"),
        ("/herald/v1", None, "herald.example"),
        ("/herald/v1/other", None, "herald.example"),
    ] {
        let headers = [("Host", host)];
        let inbound = Inbound {
            query,
            headers: &headers,
            ..get(path)
        };
        let answered = world.answer(&inbound);
        assert_eq!(answered.reply.status, 404, "{path}");
        let url = format!(
            "https://{host}{path}{}",
            query.map(|query| format!("?{query}")).unwrap_or_default()
        );
        let mut components = vec![
            Component::Status,
            Component::Field("content-type".to_owned()),
            Component::Field("content-digest".to_owned()),
        ];
        components.extend(request_parts());
        assert_eq!(signed_over(&answered, &inbound, &url), components, "{path}");
    }
    assert_eq!(world.answer(&get("/herald/v1x")).reply.status, 404);
    let posted = Inbound {
        method: "POST",
        ..get("/herald/v1/feeds/releases")
    };
    assert_eq!(world.answer(&posted).reply.status, 405);
}

#[test]
fn a_feed_the_store_cannot_read_is_not_served() {
    let mut world = world();
    world.store.fault = Fault::ReadsFeeds;
    let answered = world.answer(&get("/herald/v1/feeds/releases"));
    assert_eq!(answered.reply.status, 503);
    assert!(answered.refused.is_some());
}

#[test]
fn an_answer_that_cannot_be_signed_is_refused() {
    let unhosted = Inbound {
        headers: &[("Host", "not a host")],
        ..get("/herald/v1/other")
    };
    let answered = world().answer(&unhosted);
    assert_eq!(answered.reply.status, 500);
    assert_eq!(
        answered.refused.as_deref(),
        Some("the message's URL https://not a host/herald/v1/other names no host")
    );
    for (key, why) in [
        (
            "HERALD_KEY_RSA",
            "the federation key HERALD_KEY_RSA is not an Ed25519 key",
        ),
        (
            "BAD",
            "the federation key cannot be read: the secret BAD is not a key for this signature; give a private key as PEM, PKCS#8 or an RSA key's PKCS#1",
        ),
    ] {
        let text = format!(
            "{}\n[federation]\ndomain = \"herald.example\"\nkeys = [{{ secret = \"{key}\" }}]\n",
            config_text()
        );
        let unfed = load(
            "herald.toml",
            &text,
            &|_| Err(String::new()),
            &crate::harness::kinds(),
        )
        .unwrap();
        let world = World::with(unfed, Script::default());
        let answered = world.answer(&get("/herald/v1/other"));
        assert_eq!(answered.reply.status, 500, "{key}");
        assert_eq!(answered.refused.as_deref(), Some(why));
    }
}

#[test]
fn the_discovery_document_and_nodeinfo_name_each_feed_once() {
    let world = World::with(
        fed(
            "keys = [{ secret = \"HERALD_KEY_NEW\" }]",
            "[[routes]]\nid = \"fed-again\"\nsources = [\"quiet\"]\ndestination = \"releases\"\n",
        ),
        Script::default(),
    );
    let inbound = get("/.well-known/herald");
    let answered = world.answer(&inbound);
    let found = page(&answered);
    assert_eq!(found["features"], json!(["feeds"]));
    assert_eq!(found["feeds"], json!([{ "name": "releases", "url": FEED }]));
    signed_over(
        &answered,
        &inbound,
        "https://herald.example/.well-known/herald",
    );
    let (_, _, body) = said(&world.answer(&get("/nodeinfo/2.1")));
    let info: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(info["metadata"]["feeds"], json!([FEED]));
}
