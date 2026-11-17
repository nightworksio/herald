//! Whatever arrives from another instance, reading it never panics: a
//! structured field's items write back to what they read as, canonical
//! JSON is its own canonical form, and a key set reads back as it writes.

#![no_main]

use herald_peers::jcs::canonical;
use herald_peers::keys::{KeySet, rotation};
use herald_peers::message::{FIELD_MOST, Message, SIGNATURE, SIGNATURE_INPUT, presented};
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
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return;
    };
    if let Ok(written) = canonical(&value) {
        let read: serde_json::Value = serde_json::from_str(&written).expect("canonical JSON reads");
        assert_eq!(canonical(&read).as_deref(), Ok(written.as_str()));
    }
    let _ = shown(&value);
    if let Ok(set) = KeySet::read(&value, DOMAIN) {
        assert_eq!(KeySet::read(&set.document(DOMAIN), DOMAIN), Ok(set.clone()));
        let _ = rotation(&value, &set, DOMAIN);
    }
});
