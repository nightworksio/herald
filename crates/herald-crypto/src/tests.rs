use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::*;

const RSA: &str = include_str!("../fixtures/rsa-2048-pkcs8.test.hex");

fn pem(der: &[u8]) -> String {
    format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        STANDARD.encode(der)
    )
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.trim().len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
        .collect()
}

fn sign(algorithm: Algorithm, key: &Secret, message: &[u8]) -> Result<Vec<u8>, CryptoError> {
    pollster::block_on(RingSigner.sign(algorithm, key, message))
}

fn verify(algorithm: Algorithm, key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    pollster::block_on(RingVerifier.verify(algorithm, key, message, signature))
}

#[test]
fn an_hmac_is_rfc_4231_s_and_verifies_only_for_its_message_and_key() {
    let key = Secret::new("K", "Jefe");
    let mac = sign(Algorithm::HmacSha256, &key, b"what do ya want for nothing?").unwrap();
    assert_eq!(
        herald_core::hex(&mac),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    assert!(verify(
        Algorithm::HmacSha256,
        b"Jefe",
        b"what do ya want for nothing?",
        &mac
    ));
    assert!(!verify(
        Algorithm::HmacSha256,
        b"Jefe",
        b"something else",
        &mac
    ));
    assert!(!verify(
        Algorithm::HmacSha256,
        b"other",
        b"what do ya want for nothing?",
        &mac
    ));
}

#[test]
fn an_ed25519_signature_verifies_under_its_public_key_only() {
    let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let pair = Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap();
    let key = Secret::new("ED", pem(document.as_ref()));
    let signature = sign(Algorithm::Ed25519, &key, b"hello").unwrap();
    assert!(verify(
        Algorithm::Ed25519,
        pair.public_key().as_ref(),
        b"hello",
        &signature
    ));
    assert!(!verify(
        Algorithm::Ed25519,
        pair.public_key().as_ref(),
        b"other",
        &signature
    ));
}

#[test]
fn an_rsa_signature_verifies_under_its_public_key_only() {
    let der = unhex(RSA);
    let pair = RsaKeyPair::from_pkcs8(&der).unwrap();
    let key = Secret::new("RSA", pem(&der));
    let signature = sign(Algorithm::RsaSha256, &key, b"hello").unwrap();
    assert!(verify(
        Algorithm::RsaSha256,
        pair.public().as_ref(),
        b"hello",
        &signature
    ));
    assert!(!verify(
        Algorithm::RsaSha256,
        pair.public().as_ref(),
        b"other",
        &signature
    ));
}

#[test]
fn a_secret_that_is_not_a_key_for_the_algorithm_is_named() {
    let refused = CryptoError::BadKey {
        name: "NOT_A_KEY".to_owned(),
    };
    for (algorithm, value) in [
        (Algorithm::Ed25519, "not base64 at all!"),
        (Algorithm::Ed25519, "aGVsbG8="),
        (Algorithm::RsaSha256, "aGVsbG8="),
        (Algorithm::RsaSha256, "!!"),
    ] {
        assert_eq!(
            sign(algorithm, &Secret::new("NOT_A_KEY", value), b"m"),
            Err(refused.clone()),
            "{algorithm:?} {value}"
        );
    }
}

#[test]
fn githubs_documented_delivery_verifies_and_a_changed_body_does_not() {
    let headers = [(
        "X-Hub-Signature-256",
        "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17",
    )];
    let now = jiff::Timestamp::UNIX_EPOCH;
    let secret = b"It's a Secret to Everybody";
    for (body, holds) in [
        (b"Hello, World!".as_slice(), true),
        (b"Hello, World?", false),
    ] {
        let claim = herald_core::claim(herald_core::Scheme::GitHub, &headers, body, now).unwrap();
        assert_eq!(
            verify(
                Algorithm::HmacSha256,
                secret,
                &claim.message,
                &claim.signatures[0]
            ),
            holds
        );
    }
}

const SPKI: &str = include_str!("../fixtures/rsa-2048-spki.test.pem");

#[test]
fn an_activity_signed_by_its_actors_key_verifies_through_the_pem_it_publishes() {
    let body = b"{\"type\":\"Follow\"}";
    let digest = format!(
        "SHA-256={}",
        STANDARD.encode(ring::digest::digest(&ring::digest::SHA256, body))
    );
    let date = "Fri, 15 Jan 2027 08:00:00 GMT";
    let signing = format!(
        "(request-target): post /inbox\nhost: herald.example\ndate: {date}\ndigest: {digest}"
    );
    let key = Secret::new("ACTOR", pem(&unhex(RSA)));
    let signed = STANDARD.encode(sign(Algorithm::RsaSha256, &key, signing.as_bytes()).unwrap());
    let signature = format!(
        r#"keyId="https://social.example/users/ana#main-key",algorithm="rsa-sha256",headers="(request-target) host date digest",signature="{signed}""#
    );
    let headers = [
        ("Host", "herald.example"),
        ("Date", date),
        ("Digest", digest.as_str()),
        ("Signature", signature.as_str()),
    ];
    let now = "2027-01-15T08:00:00Z".parse().unwrap();
    let public = herald_core::key(herald_core::Scheme::ActivityPub, SPKI).unwrap();
    for (body, holds) in [(body.as_slice(), true), (b"{\"type\":\"Undo\"}", false)] {
        let to = herald_core::Addressed {
            target: "post /inbox",
            audience: "herald.example",
        };
        match herald_core::activity_claim(to, &headers, body, now) {
            Ok(claim) => assert_eq!(
                verify(
                    Algorithm::RsaSha256,
                    &public,
                    &claim.message,
                    &claim.signatures[0]
                ),
                holds
            ),
            Err(refusal) => assert!(!holds, "{refusal}"),
        }
    }
}

fn public(algorithm: Algorithm, key: &Secret) -> Result<Vec<u8>, CryptoError> {
    pollster::block_on(RingSigner.public(algorithm, key))
}

#[test]
fn a_public_key_is_published_as_a_subject_public_key_info() {
    let der = unhex(RSA);
    let rsa = public(Algorithm::RsaSha256, &Secret::new("RSA", pem(&der))).unwrap();
    assert_eq!(
        rsa,
        herald_keys::pem(SPKI, herald_keys::PUBLIC_KEY).unwrap(),
        "the key's own published PEM"
    );
    let seed = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let ed = Ed25519KeyPair::from_pkcs8(seed.as_ref()).unwrap();
    let published = public(Algorithm::Ed25519, &Secret::new("ED", pem(seed.as_ref()))).unwrap();
    assert_eq!(
        published,
        [
            &[
                0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00
            ][..],
            ed.public_key().as_ref()
        ]
        .concat()
    );
    for (algorithm, key) in [
        (Algorithm::HmacSha256, Secret::new("HMAC", "shared")),
        (Algorithm::RsaSha256, Secret::new("NOT_A_KEY", "nope")),
        (Algorithm::Ed25519, Secret::new("NOT_A_KEY", "nope")),
    ] {
        assert_eq!(
            public(algorithm, &key),
            Err(CryptoError::BadKey {
                name: key.name().to_owned()
            })
        );
    }
}
