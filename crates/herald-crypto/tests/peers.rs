//! herald's Ed25519 signer against the published vectors of RFC 9421 and
//! W3C's `eddsa-jcs-2022`, and herald's own signatures and proofs checked
//! by its verifier.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use herald_core::Algorithm;
use herald_crypto::{RingSigner, RingVerifier};
use herald_peers::digest::{CONTENT_DIGEST, content_digest};
use herald_peers::keys::{KeySet, method, rotation};
use herald_peers::message::{Component, Covered, Demands, FIELD_MOST, Message, presented};
use herald_peers::multibase::ed25519_public;
use herald_peers::proof::{shown, unproved};
use herald_ports::{Secret, Signer as _, Verifier as _};
use serde_json::json;

/// The PKCS#8 prefix of an Ed25519 private key, before its 32-byte seed.
const PKCS8_ED25519: [u8; 16] = [
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];

/// The seed of RFC 9421's `test-key-ed25519`.
const RFC9421_SEED: &str = include_str!("../fixtures/ed25519-rfc9421-seed.test.hex");
/// The seed of W3C's `eddsa-jcs-2022` example key.
const W3C_SEED: &str = include_str!("../fixtures/ed25519-w3c-seed.test.hex");

fn unhex(text: &str) -> Vec<u8> {
    herald_core::unhex(text.trim()).unwrap_or_default()
}

fn pem(der: &[u8]) -> String {
    format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        STANDARD.encode(der)
    )
}

fn seeded(name: &str, seed: &[u8]) -> Secret {
    Secret::new(name, pem(&[&PKCS8_ED25519[..], seed].concat()))
}

fn sign(key: &Secret, message: &[u8]) -> Vec<u8> {
    pollster::block_on(RingSigner::default().sign(Algorithm::Ed25519, key, message))
        .unwrap_or_default()
}

fn public(key: &Secret) -> [u8; 32] {
    pollster::block_on(RingSigner::default().public(Algorithm::Ed25519, key))
        .ok()
        .and_then(|spki| spki.get(spki.len().saturating_sub(32)..)?.try_into().ok())
        .unwrap_or_default()
}

fn verify(key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    pollster::block_on(RingVerifier.verify(Algorithm::Ed25519, key, message, signature))
}

#[test]
fn rfc_9421_s_ed25519_example_signs_as_the_rfc_shows() {
    let key = seeded("RFC9421", &unhex(RFC9421_SEED));
    let base = "\"date\": Tue, 20 Apr 2021 02:07:55 GMT\n\"@method\": POST\n\"@path\": /foo\n\"@authority\": example.com\n\"content-type\": application/json\n\"content-length\": 18\n\"@signature-params\": (\"date\" \"@method\" \"@path\" \"@authority\" \"content-type\" \"content-length\");created=1618884473;keyid=\"test-key-ed25519\"";
    assert_eq!(
        STANDARD.encode(sign(&key, base.as_bytes())),
        "wqcAqbmYJ2ji2glfAMaRy4gruYYnx2nEFN2HN6jrnDnQCK1u02Gb04v9EDgwUPiu4A0w6vuQv5lIp5WPpBKRCw=="
    );
}

fn request(fields: &[(String, String)]) -> Message<'_> {
    Message {
        method: "POST",
        url: "https://peer.example/herald/v1/inbox",
        status: None,
        fields,
    }
}

#[test]
fn a_request_herald_signs_is_checked_by_its_verifier_and_a_changed_one_is_not() {
    let key = seeded("PEER", &[7; 32]);
    let body = br#"{"identity":"x"}"#;
    let mut fields = vec![
        ("Content-Type".to_owned(), "application/json".to_owned()),
        (CONTENT_DIGEST.to_owned(), content_digest(body)),
    ];
    let covered = Covered {
        components: vec![
            Component::Method,
            Component::TargetUri,
            Component::Field(CONTENT_DIGEST.to_owned()),
        ],
        created: 1_700_000_000,
        expires: 1_700_000_300,
        nonce: Some("n-1".to_owned()),
        keyid: method("herald.example", &public(&key)),
    };
    let base = covered.base(&request(&fields)).unwrap();
    fields.extend(covered.fields(&sign(&key, &base)));
    let shown = presented(&request(&fields), FIELD_MOST).unwrap();
    let demands = Demands {
        components: &covered.components,
        single_use: true,
        now: 1_700_000_010,
        skew: 60,
        longest: 300,
    };
    assert_eq!(shown.meets(demands), Ok(()));
    assert!(verify(&public(&key), &shown.base, &shown.signature));
    let posted_elsewhere = Message {
        url: "https://peer.example/herald/v1/other",
        ..request(&fields)
    };
    let moved = presented(&posted_elsewhere, FIELD_MOST).unwrap();
    assert!(!verify(&public(&key), &moved.base, &moved.signature));
}

#[test]
fn w3c_s_eddsa_jcs_2022_example_proves_as_the_specification_shows() {
    let key = seeded("W3C", &unhex(W3C_SEED));
    assert_eq!(
        ed25519_public(&public(&key)),
        "z6MkrJVnaZkeFzdQyMZu1cgjg7k1pZZ6pvBQ7XJPt4swbTQ2"
    );
    let credential = json!({
        "@context": [
            "https://www.w3.org/ns/credentials/v2",
            "https://www.w3.org/ns/credentials/examples/v2"
        ],
        "id": "urn:uuid:58172aac-d8ba-11ed-83dd-0b3aef56cc33",
        "type": ["VerifiableCredential", "AlumniCredential"],
        "name": "Alumni Credential",
        "description": "A minimum viable example of an Alumni Credential.",
        "issuer": "https://vc.example/issuers/5678",
        "validFrom": "2023-01-01T00:00:00Z",
        "credentialSubject": {
            "id": "did:example:abcdefgh",
            "alumniOf": "The School of Examples"
        }
    });
    let ready = unproved(
        &credential,
        "did:key:z6MkrJVnaZkeFzdQyMZu1cgjg7k1pZZ6pvBQ7XJPt4swbTQ2#z6MkrJVnaZkeFzdQyMZu1cgjg7k1pZZ6pvBQ7XJPt4swbTQ2",
        "2023-02-24T23:36:38Z".parse().unwrap(),
    )
    .unwrap();
    assert_eq!(
        herald_core::hex(ready.message()),
        "66ab154f5c2890a140cb8388a22a160454f80575f6eae09e5a097cabe539a1db59b7cb6251b8991add1ce0bc83107e3db9dbbab5bd2c28f687db1a03abc92f19"
    );
    let signature = sign(&key, ready.message());
    let proved = ready.proved(&signature);
    assert_eq!(
        proved["proof"]["proofValue"],
        "z2HnFSSPPBzR36zdDgK8PbEHeXbR56YF24jwMpt3R1eHXQzJDMWS93FCzpvJpwTWd3GAVFuUfjoJdcnTMuVor51aX"
    );
    let checked = shown(&proved).unwrap();
    assert!(verify(&public(&key), &checked.message, &checked.signature));
}

#[test]
fn a_rotation_statement_signed_by_the_pinned_key_checks_out_and_one_by_another_does_not() {
    let old = seeded("OLD", &[1; 32]);
    let new = seeded("NEW", &[2; 32]);
    let pinned = KeySet {
        sequence: 1,
        refresh: 3600,
        keys: vec![public(&old)],
    };
    let next = KeySet {
        sequence: 2,
        refresh: 3600,
        keys: vec![public(&new), public(&old)],
    };
    let ready = unproved(
        &next.document("herald.example"),
        &method("herald.example", &public(&old)),
        jiff::Timestamp::UNIX_EPOCH,
    )
    .unwrap();
    let forged = ready.clone().proved(&sign(&new, ready.message()));
    let statement = ready.clone().proved(&sign(&old, ready.message()));
    let rotated = rotation(&statement, &pinned, "herald.example").unwrap();
    assert!(verify(
        &rotated.by,
        &rotated.proof.message,
        &rotated.proof.signature
    ));
    assert_eq!(rotated.set, next);
    let claimed = shown(&forged).unwrap();
    assert!(!verify(&rotated.by, &claimed.message, &claimed.signature));
}
