//! herald's ActivityPub actors as the server answers for them: WebFinger,
//! their documents, Notes, outboxes and followers.

use herald_core::{Key, RouteId};
use herald_flows::{Answered, Inbound};
use herald_ports::Notes;

use crate::harness::{Fault, Script};
use crate::serving::{ACTOR, JSON, World, config_text, get, key, said};

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
#[test]
fn a_note_is_served_by_its_id_where_its_actor_published_it() {
    let world = World::new(Script::default());
    let key = key();
    let path = format!("/users/php-mutation-gate/notes/{key}");
    assert_eq!(world.answer(&get(&path)).reply.status, 404);
    let note = format!(r#"{{"id":"{ACTOR}/notes/{key}","type":"Note"}}"#);
    pollster::block_on(
        world
            .store
            .inner
            .put_note("herald", &key, &note, crate::harness::NOON),
    )
    .unwrap();
    assert_eq!(said(&world.answer(&get(&path))), (200, JSON, note));
    let other = format!(r#"{{"id":"https://herald.example/users/other/notes/{key}"}}"#);
    pollster::block_on(
        world
            .store
            .inner
            .put_note("herald", &key, &other, crate::harness::NOON),
    )
    .unwrap();
    assert_eq!(world.answer(&get(&path)).reply.status, 404);
    pollster::block_on(world.store.inner.put_note(
        "herald",
        &key,
        "not json",
        crate::harness::NOON,
    ))
    .unwrap();
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

fn paged<'a>(path: &'a str, query: &'a str) -> Inbound<'a> {
    Inbound {
        query: Some(query),
        ..get(path)
    }
}

fn json(answered: &Answered) -> serde_json::Value {
    serde_json::from_slice(&answered.reply.body).unwrap()
}

fn published(world: &World, number: i64) -> Key {
    let key = Key::derive(
        &RouteId::new("followed"),
        &herald_core::Identity::new(number.to_string()),
    );
    let note = format!(
        r#"{{"id":"{ACTOR}/notes/{key}","type":"Note","attributedTo":"{ACTOR}","published":"{number}","to":["t"],"cc":["c"]}}"#
    );
    let at = crate::harness::NOON + jiff::SignedDuration::from_hours(number);
    pollster::block_on(
        world
            .store
            .inner
            .put_note("php-mutation-gate", &key, &note, at),
    )
    .unwrap();
    key
}

#[test]
fn an_actor_s_outbox_counts_its_notes_and_pages_them_newest_first_as_creates() {
    let world = World::new(Script::default());
    let outbox = "/users/php-mutation-gate/outbox";
    let empty = world.answer(&get(outbox));
    assert_eq!((empty.reply.status, empty.reply.kind), (200, JSON));
    assert_eq!(
        json(&empty),
        serde_json::json!({
            "@context": "https://www.w3.org/ns/activitystreams",
            "id": format!("{ACTOR}/outbox"),
            "type": "OrderedCollection",
            "totalItems": 0,
            "first": format!("{ACTOR}/outbox?page=true"),
        })
    );
    let keys: Vec<Key> = (1..=3).map(|number| published(&world, number)).collect();
    pollster::block_on(world.store.inner.put_note(
        "php-mutation-gate",
        &Key::derive(&RouteId::new("followed"), &herald_core::Identity::new("x")),
        r#"{"id":"https://herald.example/users/deploys/notes/x","attributedTo":"https://herald.example/users/deploys"}"#,
        crate::harness::NOON,
    ))
    .unwrap();
    assert_eq!(json(&world.answer(&get(outbox)))["totalItems"], 4);
    let first = json(&world.answer(&paged(outbox, "page=true")));
    assert_eq!(first["type"], "OrderedCollectionPage");
    assert_eq!(first["id"], format!("{ACTOR}/outbox?page=true"));
    assert_eq!(first["partOf"], format!("{ACTOR}/outbox"));
    let items = first["orderedItems"].as_array().unwrap();
    let ids: Vec<&str> = items
        .iter()
        .map(|item| item["object"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            format!("{ACTOR}/notes/{}", keys[2]),
            format!("{ACTOR}/notes/{}", keys[1])
        ]
    );
    assert_eq!(items[0]["type"], "Create");
    assert_eq!(
        items[0]["id"],
        format!("{ACTOR}/notes/{}/activity", keys[2])
    );
    assert_eq!(items[0]["actor"], ACTOR);
    assert_eq!(items[0]["published"], "3");
    assert_eq!(
        (&items[0]["to"], &items[0]["cc"]),
        (&serde_json::json!(["t"]), &serde_json::json!(["c"]))
    );
    let next = first["next"].as_str().unwrap();
    let query = next.split_once('?').unwrap().1;
    let older = json(&world.answer(&paged(outbox, query)));
    let older_ids: Vec<&str> = older["orderedItems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["object"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(older_ids, [format!("{ACTOR}/notes/{}", keys[0])]);
    assert!(older.get("next").is_none(), "{older}");
    assert_eq!(world.answer(&paged(outbox, "page=soon")).reply.status, 404);
    assert_eq!(
        world
            .answer(&paged(
                outbox,
                &format!("page=00000000000000000001.{}", keys[0])
            ))
            .reply
            .status,
        200
    );
    assert_eq!(
        world
            .answer(&paged(outbox, "other=1&page=true"))
            .reply
            .status,
        200
    );
    let posted = Inbound {
        method: "POST",
        ..get(outbox)
    };
    assert_eq!(world.answer(&posted).reply.status, 405);
    let unread = World::failing(Fault::ReadsNotes);
    assert_eq!(unread.answer(&get(outbox)).reply.status, 503);
}

#[test]
fn an_actor_s_followers_are_counted_and_never_listed() {
    let world = World::new(Script::default());
    let followers = "/users/php-mutation-gate/followers";
    assert_eq!(json(&world.answer(&get(followers)))["totalItems"], 0);
    for follower in ["https://a.example/users/ana", "https://b.example/users/bo"] {
        pollster::block_on(herald_ports::Subscriptions::subscribe(
            &world.store.inner,
            &herald_core::Subscription {
                destination: herald_core::DestinationName::new("fedi"),
                project: "php-mutation-gate".to_owned(),
                subscriber: follower.to_owned(),
                inbox: None,
                pending: None,
            },
        ))
        .unwrap();
    }
    let counted = world.answer(&get(followers));
    assert_eq!((counted.reply.status, counted.reply.kind), (200, JSON));
    assert_eq!(
        json(&counted),
        serde_json::json!({
            "@context": "https://www.w3.org/ns/activitystreams",
            "id": format!("{ACTOR}/followers"),
            "type": "OrderedCollection",
            "totalItems": 2,
        })
    );
    assert_eq!(
        json(&world.answer(&get("/users/deploys/followers")))["totalItems"],
        0
    );
    let posted = Inbound {
        method: "POST",
        ..get(followers)
    };
    assert_eq!(world.answer(&posted).reply.status, 405);
    let unread = World::failing(Fault::Subscribers);
    assert_eq!(unread.answer(&get(followers)).reply.status, 503);
    let text = config_text().replace(
        "outbox_page = 2\n",
        "outbox_page = 2\nblock = [\"b.example\"]\n",
    );
    let blocking = World::with(
        herald_config::load(
            "herald.toml",
            &text,
            &|_| Err(String::new()),
            &crate::harness::kinds(),
        )
        .unwrap(),
        Script::default(),
    );
    pollster::block_on(herald_ports::Subscriptions::subscribe(
        &blocking.store.inner,
        &herald_core::Subscription {
            destination: herald_core::DestinationName::new("fedi"),
            project: "php-mutation-gate".to_owned(),
            subscriber: "https://b.example/users/bo".to_owned(),
            inbox: None,
            pending: None,
        },
    ))
    .unwrap();
    assert_eq!(json(&blocking.answer(&get(followers)))["totalItems"], 0);
    pollster::block_on(herald_ports::Subscriptions::subscribe(
        &blocking.store.inner,
        &herald_core::Subscription {
            destination: herald_core::DestinationName::new("fedi"),
            project: "php-mutation-gate".to_owned(),
            subscriber: "https://c.example/users/cy".to_owned(),
            inbox: None,
            pending: Some("https://c.example/f/1".to_owned()),
        },
    ))
    .unwrap();
    assert_eq!(json(&blocking.answer(&get(followers)))["totalItems"], 0);
}
