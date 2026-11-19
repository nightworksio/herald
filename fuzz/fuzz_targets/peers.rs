//! Whatever arrives from another instance, reading it never panics: a
//! structured field's items write back to what they read as, canonical
//! JSON is its own canonical form, a key set reads back as it writes, and
//! an answer, a discovery document, a feed's page, a static log's listing
//! or signature, or an envelope is checked or read without panicking.

#![no_main]

use herald_core::Visibility;
use herald_peers::answer::{Answer, Bounds, CONTENT_TYPE, checked};
use herald_peers::digest::CONTENT_DIGEST;
use herald_peers::discovery::{handed_over, key_set};
use herald_peers::reading::{carried, page};
use herald_peers::jcs::canonical;
use herald_peers::keys::{KeySet, rotation};
use herald_peers::message::{FIELD_MOST, Message, SIGNATURE, SIGNATURE_INPUT, presented};
use herald_peers::pages::{Log, listing, signed};
use herald_peers::proof::shown;
use herald_peers::structured::{Item, Member, dictionary, written};
use libfuzzer_sys::fuzz_target;

const DOMAIN: &str = "herald.example";

fuzz_target!(|bytes: &[u8]| {
    let text = String::from_utf8_lossy(bytes);
    for (_, member) in dictionary(&text).unwrap_or_default() {
        if let Member::Item(Item { bare, .. }) = member {
            let field = format!("a={}", written(&bare));
            assert_eq!(
                dictionary(&field),
                Some(vec![(
                    "a".to_owned(),
                    Member::Item(Item {
                        bare,
                        parameters: Vec::new()
                    })
                )])
            );
        }
    }
    let fields = vec![
        (SIGNATURE_INPUT.to_owned(), text.to_string()),
        (SIGNATURE.to_owned(), text.to_string()),
    ];
    let _ = presented(&Message {
        method: "POST",
        url: "https://herald.example/herald/v1/inbox",
        status: Some(200),
        fields: &fields,
    }, FIELD_MOST);
    let answered = vec![
        (CONTENT_TYPE.to_owned(), text.to_string()),
        (CONTENT_DIGEST.to_owned(), text.to_string()),
        (SIGNATURE_INPUT.to_owned(), text.to_string()),
        (SIGNATURE.to_owned(), text.to_string()),
    ];
    let answer = Answer {
        method: "GET",
        url: "https://herald.example/.well-known/herald",
        status: 200,
        fields: &answered,
        body: bytes,
    };
    let bounds = Bounds { fields: FIELD_MOST, now: 1_800_000_000, skew: 300, longest: 1200 };
    let _ = checked(&answer, DOMAIN, &[[0; 32]], bounds);
    let _ = key_set(bytes, DOMAIN);
    let held = KeySet { sequence: 0, refresh: 0, keys: vec![[0; 32]] };
    let _ = handed_over(bytes, &held, DOMAIN);
    let feed = "https://herald.example/herald/v1/feeds/releases";
    let _ = signed(bytes, "https://herald.example/.well-known/herald", b"{}");
    if let Ok(log) = signed(bytes, "https://herald.example/.well-known/herald", bytes).map(|read| read.log) {
        let _ = listing(bytes, log, feed);
    }
    let _ = listing(bytes, Log { sequence: 1, published: jiff::Timestamp::UNIX_EPOCH }, feed);
    if let Ok(read) = page(bytes, "https://herald.example/herald/v1/feeds/releases") {
        for (_, envelope) in &read.items {
            let _ = carried(envelope, Visibility::Public);
        }
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return;
    };
    if let Ok(written) = canonical(&value) {
        let read: serde_json::Value = serde_json::from_str(&written).expect("canonical JSON reads");
        assert_eq!(canonical(&read).as_deref(), Ok(written.as_str()));
    }
    let _ = shown(&value);
    let _ = carried(&value, Visibility::Private);
    if let Ok(set) = KeySet::read(&value, DOMAIN) {
        assert_eq!(KeySet::read(&set.document(DOMAIN), DOMAIN), Ok(set.clone()));
        let _ = rotation(&value, &set, DOMAIN);
    }
});
