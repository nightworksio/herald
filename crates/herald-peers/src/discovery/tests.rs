use serde_json::{Value, json};

use super::*;

const INSTANCE: Instance<'static> = Instance {
    domain: "herald.example",
    base: "/herald",
    software: "herald",
    version: "1.2.3",
    repository: "https://github.com/nightworksio/herald",
};

fn read(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn the_discovery_document_names_the_api_the_software_and_the_key_set() {
    let keys = KeySet {
        sequence: 2,
        refresh: 86_400,
        keys: vec![[1; 32]],
    };
    assert_eq!(
        read(&INSTANCE.document(&keys.document("herald.example"), &[])),
        json!({
            "version": 1,
            "api": "https://herald.example/herald/herald/v1",
            "software": { "name": "herald", "version": "1.2.3" },
            "keys": keys.document("herald.example"),
            "features": [],
            "feeds": [],
        })
    );
    assert_eq!(
        INSTANCE.discovery(),
        "https://herald.example/herald/.well-known/herald"
    );
    let feeding = read(&INSTANCE.document(&keys.document("herald.example"), &["releases", "tags"]));
    assert_eq!(feeding["features"], json!(["feeds"]));
    assert_eq!(
        feeding["feeds"],
        json!([
            { "name": "releases", "url": "https://herald.example/herald/herald/v1/feeds/releases" },
            { "name": "tags", "url": "https://herald.example/herald/herald/v1/feeds/tags" },
        ])
    );
}

#[test]
fn webfinger_finds_the_instance_by_its_account_or_its_domain() {
    for named in [
        "acct:herald@herald.example",
        "https://herald.example",
        "https://herald.example/",
    ] {
        assert!(INSTANCE.named_by(named), "{named}");
    }
    for other in [
        "acct:gate@herald.example",
        "acct:herald@other.example",
        "https://herald.example/users/gate",
        "",
    ] {
        assert!(!INSTANCE.named_by(other), "{other}");
    }
    assert_eq!(
        read(&INSTANCE.finger()),
        json!({
            "subject": "acct:herald@herald.example",
            "aliases": ["https://herald.example/"],
            "links": [{
                "rel": "self",
                "type": "application/json",
                "href": "https://herald.example/herald/.well-known/herald",
            }],
        })
    );
}

#[test]
fn nodeinfo_links_its_document_and_names_activitypub_only_where_actors_are_served() {
    assert_eq!(
        read(&INSTANCE.nodeinfo_links()),
        json!({ "links": [{
            "rel": "http://nodeinfo.diaspora.software/ns/schema/2.1",
            "href": "https://herald.example/herald/nodeinfo/2.1",
        }] })
    );
    let served = read(&INSTANCE.nodeinfo(3, &["releases"]));
    assert_eq!(
        served,
        json!({
            "version": "2.1",
            "software": {
                "name": "herald",
                "version": "1.2.3",
                "repository": "https://github.com/nightworksio/herald",
            },
            "protocols": ["activitypub"],
            "services": { "inbound": [], "outbound": [] },
            "openRegistrations": false,
            "usage": { "users": { "total": 3 } },
            "metadata": {
                "discovery": "https://herald.example/herald/.well-known/herald",
                "feeds": ["https://herald.example/herald/herald/v1/feeds/releases"],
            },
        })
    );
    let none = read(&INSTANCE.nodeinfo(0, &[]));
    assert_eq!(none["protocols"], json!([]));
    assert_eq!(none["usage"]["users"]["total"], json!(0));
    assert_eq!(none["metadata"]["feeds"], json!([]));
}

#[test]
fn the_key_set_a_discovery_document_lists_reads_back_as_written() {
    let keys = KeySet {
        sequence: 4,
        refresh: 3_600,
        keys: vec![[5; 32], [6; 32]],
    };
    let proved = json!({ "proof": { "type": "DataIntegrityProof" } });
    let mut written = keys.document("herald.example");
    written["proof"] = proved["proof"].clone();
    let document = INSTANCE.document(&written, &[]);
    assert_eq!(super::keys(document.as_bytes()), Ok(written));
    assert_eq!(super::keys(br#"{"version": 1}"#), Ok(Value::Null));
    assert_eq!(key_set(document.as_bytes(), "herald.example"), Ok(keys));
    assert!(matches!(
        key_set(document.as_bytes(), "other.example"),
        Err(Unkeyed::Foreign(_))
    ));
    assert_eq!(
        key_set(br#"{"version": 1}"#, "herald.example"),
        Err(Unkeyed::Shape("it has no sequence"))
    );
    assert_eq!(
        key_set(b"<html>", "herald.example"),
        Err(Unkeyed::Shape("it is no JSON"))
    );
}

#[test]
fn an_instance_is_reached_at_an_https_domain_alone() {
    for url in [
        "http://herald.example",
        "https://herald.example:8443",
        "https://127.0.0.1",
        "https://localhost",
        "https://user@herald.example",
        "https://herald.example/?page=1",
        "https://herald.example/#top",
        "herald.example",
    ] {
        assert_eq!(address(url), None, "{url}");
    }
    let at = |url: &str, domain: &str| Some((url.to_owned(), domain.to_owned()));
    assert_eq!(
        address("https://herald.example"),
        at("https://herald.example", "herald.example")
    );
    assert_eq!(
        address("https://herald.example/under/"),
        at("https://herald.example/under", "herald.example")
    );
}

#[test]
fn a_discovery_document_s_key_set_reads_as_a_rotation_statement() {
    let held = KeySet {
        sequence: 1,
        refresh: 60,
        keys: vec![[5; 32]],
    };
    let next = KeySet {
        sequence: 2,
        refresh: 60,
        keys: vec![[6; 32], [5; 32]],
    };
    let unproved = crate::proof::unproved(
        &next.document("herald.example"),
        &crate::keys::method("herald.example", &[5; 32]),
        jiff::Timestamp::UNIX_EPOCH,
    )
    .unwrap();
    let proved = unproved.clone().proved(&[1; 64]);
    let document = INSTANCE.document(&proved, &[]);
    let rotated = handed_over(document.as_bytes(), &held, "herald.example").unwrap();
    assert_eq!((rotated.set, rotated.by), (next, [5; 32]));
    assert_eq!(rotated.proof.message, unproved.message());
    assert_eq!(
        handed_over(b"<html>", &held, "herald.example"),
        Err(Unrotated::Unkeyed(Unkeyed::Shape("it is no JSON")))
    );
}
