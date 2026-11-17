//! The instance as other heralds find it: its discovery document with its
//! key set under a sequence the store keeps, its WebFinger account, and
//! NodeInfo.

use herald_peers::multibase::ed25519_public;
use herald_ports::Published as _;

use super::*;

fn federated(keys: &[&str]) -> Config {
    let keys: Vec<String> = keys
        .iter()
        .map(|key| format!("{{ secret = \"{key}\" }}"))
        .collect();
    let text = format!(
        "{}\n[federation]\ndomain = \"herald.example\"\nkeys = [{}]\n",
        config_text(),
        keys.join(", ")
    );
    load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap()
}

fn multikey(key: &str) -> String {
    ed25519_public(&key.as_bytes().try_into().unwrap())
}

fn document(world: &World) -> serde_json::Value {
    let (status, kind, body) = said(&world.answer(&get("/.well-known/herald")));
    assert_eq!((status, kind), (200, "application/json"), "{body}");
    serde_json::from_str(&body).unwrap()
}

fn listed(document: &serde_json::Value) -> Vec<String> {
    document["keys"]["keys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| key["publicKeyMultibase"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn the_discovery_document_lists_the_keys_newest_first_under_the_first_sequence() {
    let world = World::with(
        federated(&["HERALD_KEY_NEW", "HERALD_KEY_OLD"]),
        Script::default(),
    );
    let found = document(&world);
    assert_eq!(found["version"], 1);
    assert_eq!(found["api"], "https://herald.example/herald/v1");
    assert_eq!(found["software"]["name"], "herald");
    assert_eq!(found["keys"]["sequence"], 1);
    assert_eq!(found["keys"]["refresh"], 86_400);
    assert_eq!(listed(&found), [multikey(NEW_KEY), multikey(OLD_KEY)]);
    let kept = pollster::block_on(world.store.inner.published_keys())
        .unwrap()
        .unwrap();
    assert_eq!(kept.value.keys, [multikey(NEW_KEY), multikey(OLD_KEY)]);
    assert_eq!(document(&world)["keys"]["sequence"], 1);
    let again = pollster::block_on(world.store.inner.published_keys())
        .unwrap()
        .unwrap();
    assert_eq!(again.version, kept.version);
}

#[test]
fn a_new_set_of_keys_is_published_under_the_next_sequence() {
    let first = World::with(federated(&["HERALD_KEY_OLD"]), Script::default());
    assert_eq!(document(&first)["keys"]["sequence"], 1);
    let mut rotated = World::with(
        federated(&["HERALD_KEY_NEW", "HERALD_KEY_OLD"]),
        Script::default(),
    );
    rotated.store = first.store;
    let found = document(&rotated);
    assert_eq!(found["keys"]["sequence"], 2);
    assert_eq!(listed(&found), [multikey(NEW_KEY), multikey(OLD_KEY)]);
    let kept = pollster::block_on(rotated.store.inner.published_keys())
        .unwrap()
        .unwrap();
    assert_eq!(kept.value.sequence, 2);
    assert_eq!(kept.value.keys, [multikey(NEW_KEY), multikey(OLD_KEY)]);
}

#[test]
fn a_store_that_keeps_no_key_sets_publishes_under_sequence_zero() {
    let mut world = World::with(federated(&["HERALD_KEY_NEW"]), Script::default());
    world.store.fault = Fault::KeepsNoPublished;
    assert_eq!(document(&world)["keys"]["sequence"], 0);
}

#[test]
fn a_key_set_the_store_cannot_read_or_write_is_not_served() {
    for fault in [
        Fault::ReadsPublished,
        Fault::WritesPublished,
        Fault::ConflictsPublished,
    ] {
        let mut world = World::with(federated(&["HERALD_KEY_NEW"]), Script::default());
        world.store.fault = fault;
        let answered = world.answer(&get("/.well-known/herald"));
        assert_eq!(answered.reply.status, 503, "{fault:?}");
        assert!(answered.refused.is_some(), "{fault:?}");
    }
    let mut world = World::with(federated(&["HERALD_KEY_NEW"]), Script::default());
    world.store.fault = Fault::ConflictsPublished;
    assert_eq!(
        world.answer(&get("/.well-known/herald")).refused.as_deref(),
        Some(
            "the store could not be reached: the key set was written by another run each of 3 times herald wrote it"
        )
    );
}

#[test]
fn a_federation_key_that_is_no_ed25519_key_or_cannot_be_read_is_not_served() {
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
        let world = World::with(federated(&[key]), Script::default());
        let answered = world.answer(&get("/.well-known/herald"));
        assert_eq!(answered.reply.status, 500, "{key}");
        assert_eq!(answered.refused.as_deref(), Some(why));
    }
}

#[test]
fn another_host_or_a_config_without_a_domain_has_no_instance() {
    let world = World::with(federated(&["HERALD_KEY_NEW"]), Script::default());
    for path in [
        "/.well-known/herald",
        "/.well-known/nodeinfo",
        "/nodeinfo/2.1",
    ] {
        let elsewhere = Inbound {
            headers: &[("Host", "other.example")],
            ..get(path)
        };
        assert_eq!(world.answer(&elsewhere).reply.status, 404, "{path}");
        assert_eq!(
            World::new(Script::default())
                .answer(&get(path))
                .reply
                .status,
            404,
            "{path}"
        );
        let posted = Inbound {
            method: "POST",
            ..get(path)
        };
        assert_eq!(world.answer(&posted).reply.status, 405, "{path}");
    }
}

#[test]
fn webfinger_finds_the_instance_by_its_account_or_its_domain() {
    let world = World::with(federated(&["HERALD_KEY_NEW"]), Script::default());
    for query in [
        "resource=acct%3Aherald%40herald.example",
        "resource=https%3A%2F%2Fherald.example%2F",
    ] {
        let asked = Inbound {
            query: Some(query),
            ..get("/.well-known/webfinger")
        };
        let (status, kind, body) = said(&world.answer(&asked));
        assert_eq!((status, kind), (200, "application/jrd+json"), "{query}");
        let found: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(found["subject"], "acct:herald@herald.example");
        assert_eq!(
            found["links"][0]["href"],
            "https://herald.example/.well-known/herald"
        );
    }
    let unfederated = World::new(Script::default());
    let asked = Inbound {
        query: Some("resource=acct%3Aherald%40herald.example"),
        ..get("/.well-known/webfinger")
    };
    assert_eq!(unfederated.answer(&asked).reply.status, 404);
}

#[test]
fn nodeinfo_links_its_document_and_counts_the_actors_on_the_domain() {
    let world = World::with(federated(&["HERALD_KEY_NEW"]), Script::default());
    let (status, kind, body) = said(&world.answer(&get("/.well-known/nodeinfo")));
    assert_eq!((status, kind), (200, "application/json"));
    let links: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        links["links"][0]["href"],
        "https://herald.example/nodeinfo/2.1"
    );
    let (status, kind, body) = said(&world.answer(&get("/nodeinfo/2.1")));
    assert_eq!(
        (status, kind),
        (
            200,
            "application/json; profile=\"http://nodeinfo.diaspora.software/ns/schema/2.1#\""
        )
    );
    let info: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(info["usage"]["users"]["total"], 3);
    assert_eq!(info["protocols"], serde_json::json!(["activitypub"]));
    let text = format!(
        "{}\n[federation]\ndomain = \"herald.example\"\n",
        config_text().replace("domain = \"herald.example\"", "domain = \"actors.example\"")
    );
    let elsewhere = load(
        "herald.toml",
        &text,
        &|_| Err(String::new()),
        &crate::harness::kinds(),
    )
    .unwrap();
    let (_, _, body) =
        said(&World::with(elsewhere, Script::default()).answer(&get("/nodeinfo/2.1")));
    let info: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(info["usage"]["users"]["total"], 0);
    assert_eq!(info["protocols"], serde_json::json!([]));
}

#[test]
fn the_key_set_is_proved_by_its_oldest_key_so_a_peer_pinning_it_takes_the_set() {
    let world = World::with(
        federated(&["HERALD_KEY_NEW", "HERALD_KEY_OLD"]),
        Script::default(),
    );
    let found = document(&world);
    let proof = &found["keys"]["proof"];
    let oldest: [u8; 32] = OLD_KEY.as_bytes().try_into().unwrap();
    assert_eq!(
        proof["verificationMethod"],
        herald_peers::keys::method("herald.example", &oldest)
    );
    let signature =
        herald_peers::multibase::decode(proof["proofValue"].as_str().unwrap(), 4096).unwrap();
    assert!(signature.starts_with(&[b"Ed25519|".as_slice(), OLD_KEY.as_bytes(), b"|"].concat()));
}
