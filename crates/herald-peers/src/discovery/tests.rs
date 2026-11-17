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
        read(&INSTANCE.document(&keys)),
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
    let served = read(&INSTANCE.nodeinfo(3));
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
                "feeds": [],
            },
        })
    );
    let none = read(&INSTANCE.nodeinfo(0));
    assert_eq!(none["protocols"], json!([]));
    assert_eq!(none["usage"]["users"]["total"], json!(0));
}
