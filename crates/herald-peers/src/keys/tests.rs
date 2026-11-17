use serde_json::json;

use super::*;
use crate::proof::unproved;

const DOMAIN: &str = "herald.example";
const OLD: Key = [1; 32];
const NEW: Key = [2; 32];

fn set(sequence: u64, keys: &[Key]) -> KeySet {
    KeySet {
        sequence,
        refresh: 3600,
        keys: keys.to_vec(),
    }
}

#[test]
fn a_key_is_named_by_the_discovery_document_and_its_multikey() {
    assert_eq!(
        controller(DOMAIN),
        "https://herald.example/.well-known/herald"
    );
    assert_eq!(
        method(DOMAIN, &OLD),
        format!(
            "https://herald.example/.well-known/herald#{}",
            ed25519_public(&OLD)
        )
    );
}

#[test]
fn the_newest_key_signs_and_every_key_is_named() {
    let both = set(2, &[NEW, OLD]);
    assert_eq!(both.signing(), Some(&NEW));
    assert_eq!(both.named(DOMAIN, &method(DOMAIN, &OLD)), Some(&OLD));
    assert_eq!(both.named("other.example", &method(DOMAIN, &OLD)), None);
    assert_eq!(set(3, &[]).signing(), None);
}

#[test]
fn a_set_reads_back_as_its_document_writes_it() {
    let both = set(2, &[NEW, OLD]);
    let document = both.document(DOMAIN);
    assert_eq!(
        document.get("keys").and_then(|keys| keys.get(0)),
        Some(&json!({
            "id": method(DOMAIN, &NEW),
            "type": "Multikey",
            "controller": controller(DOMAIN),
            "publicKeyMultibase": ed25519_public(&NEW),
        }))
    );
    assert_eq!(document.get("sequence"), Some(&json!(2)));
    assert_eq!(document.get("refresh"), Some(&json!(3600)));
    assert_eq!(KeySet::read(&document, DOMAIN), Ok(both));
    assert_eq!(
        KeySet::read(&set(4, &[]).document(DOMAIN), DOMAIN),
        Ok(set(4, &[]))
    );
}

#[test]
fn a_set_of_another_shape_or_instance_is_refused_by_what_is_wrong() {
    let written = set(2, &[NEW]).document(DOMAIN);
    let changed = |path: &[&str], value: Option<Value>| {
        let mut document = written.clone();
        let mut at = &mut document;
        let (last, parents) = path.split_last().unwrap();
        for parent in parents {
            at = match parent.parse::<usize>() {
                Ok(index) => &mut at[index],
                Err(_) => &mut at[*parent],
            };
        }
        let members = at.as_object_mut().unwrap();
        match value {
            Some(value) => members.insert((*last).to_owned(), value),
            None => members.remove(*last),
        };
        document
    };
    for (document, refused) in [
        (
            changed(&["sequence"], None),
            Unkeyed::Shape("it has no sequence"),
        ),
        (
            changed(&["sequence"], Some(json!(-1))),
            Unkeyed::Shape("it has no sequence"),
        ),
        (
            changed(&["refresh"], Some(json!("1h"))),
            Unkeyed::Shape("it has no refresh"),
        ),
        (
            changed(&["keys"], Some(json!({}))),
            Unkeyed::Shape("it lists no keys"),
        ),
        (
            changed(&["keys", "0", "type"], Some(json!("JsonWebKey"))),
            Unkeyed::Shape("a key is not a Multikey"),
        ),
        (
            changed(
                &["keys", "0", "publicKeyMultibase"],
                Some(json!("z2NEpo7TZRRrLZSi2U")),
            ),
            Unkeyed::Shape("a key is not an Ed25519 Multikey"),
        ),
        (
            changed(
                &["keys", "0", "id"],
                Some(json!("https://evil.example/.well-known/herald#k")),
            ),
            Unkeyed::Foreign("https://evil.example/.well-known/herald#k".to_owned()),
        ),
        (
            changed(&["keys", "0", "id"], None),
            Unkeyed::Foreign(String::new()),
        ),
        (
            changed(
                &["keys", "0", "controller"],
                Some(json!("https://evil.example/.well-known/herald")),
            ),
            Unkeyed::Foreign(method(DOMAIN, &NEW)),
        ),
    ] {
        assert_eq!(KeySet::read(&document, DOMAIN), Err(refused));
    }
    assert_eq!(
        KeySet::read(&written, "other.example"),
        Err(Unkeyed::Foreign(method(DOMAIN, &NEW)))
    );
}

fn statement(next: &KeySet, by: &Key) -> Value {
    unproved(
        &next.document(DOMAIN),
        &method(DOMAIN, by),
        jiff::Timestamp::UNIX_EPOCH,
    )
    .unwrap()
    .proved(&[7; crate::proof::SIGNATURE_LENGTH])
}

#[test]
fn a_newer_set_signed_by_a_pinned_key_is_a_rotation_whose_signature_is_left_to_check() {
    let pinned = set(1, &[OLD]);
    let next = set(2, &[NEW, OLD]);
    let document = statement(&next, &OLD);
    let rotated = rotation(&document, &pinned, DOMAIN).unwrap();
    assert_eq!(rotated.set, next);
    assert_eq!(rotated.by, OLD);
    assert_eq!(rotated.proof, shown(&document).unwrap());
    assert_eq!(
        rotation(&statement(&set(2, &[]), &OLD), &pinned, DOMAIN).map(|rotated| rotated.set),
        Ok(set(2, &[]))
    );
}

#[test]
fn a_rotation_by_a_stranger_an_old_sequence_or_an_unreadable_statement_is_refused() {
    let pinned = set(5, &[OLD]);
    for (document, refused) in [
        (
            statement(&set(6, &[NEW]), &NEW),
            Unrotated::Stranger(method(DOMAIN, &NEW)),
        ),
        (
            statement(&set(5, &[NEW]), &OLD),
            Unrotated::Stale {
                sequence: 5,
                pinned: 5,
            },
        ),
        (
            set(6, &[NEW]).document(DOMAIN),
            Unrotated::Unproven(Unproven::Unproved),
        ),
        (
            {
                let mut document = statement(&set(6, &[NEW]), &OLD);
                document["keys"] = json!(1);
                document
            },
            Unrotated::Unkeyed(Unkeyed::Shape("it lists no keys")),
        ),
    ] {
        assert_eq!(rotation(&document, &pinned, DOMAIN), Err(refused));
    }
}

#[test]
fn every_refusal_reads_as_a_sentence() {
    for (refused, text) in [
        (
            Unrotated::Unproven(Unproven::Unproved),
            "the object carries no proof",
        ),
        (
            Unrotated::Unkeyed(Unkeyed::Shape("it lists no keys")),
            "the key set cannot be read: it lists no keys",
        ),
        (
            Unrotated::Unkeyed(Unkeyed::Foreign("k".to_owned())),
            "the key k is not the instance's own",
        ),
        (
            Unrotated::Stranger("k".to_owned()),
            "the new key set is signed by k, which is not pinned",
        ),
        (
            Unrotated::Stale {
                sequence: 1,
                pinned: 2,
            },
            "the new key set's sequence 1 is not past the pinned set's 2",
        ),
    ] {
        assert_eq!(refused.to_string(), text);
    }
}

#[test]
fn a_key_s_fingerprint_is_the_sha_256_of_its_bytes() {
    assert_eq!(
        fingerprint(&[0; 32]),
        "sha256:66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925"
    );
    assert_ne!(fingerprint(&[1; 32]), fingerprint(&[0; 32]));
}
