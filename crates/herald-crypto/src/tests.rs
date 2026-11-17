use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::*;

/// A scheme as its crate writes it, for these tests.
const GITHUB: herald_core::Scheme = herald_core::Scheme {
    name: "github",
    signed: herald_core::Signed::Body {
        headers: &["X-Hub-Signature-256"],
        prefix: "sha256=",
    },
    key: herald_core::KeyForm::Bytes,
};

/// A scheme as its crate writes it, for these tests.
const ACTIVITYPUB: herald_core::Scheme = herald_core::Scheme {
    name: "activitypub",
    signed: herald_core::Signed::HttpSignature,
    key: herald_core::KeyForm::RsaPem,
};

const RSA: &str = include_str!("../fixtures/rsa-2048-pkcs8.test.hex");
const ROTATED: &str = include_str!("../fixtures/rsa-2048-pkcs8-rotated.test.hex");

fn pem(der: &[u8]) -> String {
    format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        STANDARD.encode(der)
    )
}

fn unhex(text: &str) -> Vec<u8> {
    herald_core::unhex(text.trim()).unwrap()
}

fn sign(algorithm: Algorithm, key: &Secret, message: &[u8]) -> Result<Vec<u8>, CryptoError> {
    pollster::block_on(RingSigner::default().sign(algorithm, key, message))
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
fn an_rsa_key_github_hands_out_as_pkcs1_signs_as_its_pkcs8_does() {
    let der = unhex(RSA);
    let (inner, _) = herald_core::der::element(&der, herald_core::der::SEQUENCE).unwrap();
    let (_, inner) = herald_core::der::element(inner, herald_core::der::INTEGER).unwrap();
    let (_, inner) = herald_core::der::element(inner, herald_core::der::SEQUENCE).unwrap();
    let (pkcs1, _) = herald_core::der::element(inner, herald_core::der::OCTET_STRING).unwrap();
    let armoured = format!(
        "-----BEGIN RSA PRIVATE KEY-----\n{}\n-----END RSA PRIVATE KEY-----\n",
        STANDARD.encode(pkcs1)
    );
    let pkcs8 = Secret::new("PKCS8", pem(&der));
    let github = Secret::new("PKCS1", armoured);
    assert_eq!(
        sign(Algorithm::RsaSha256, &github, b"hello").unwrap(),
        sign(Algorithm::RsaSha256, &pkcs8, b"hello").unwrap()
    );
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
        let claim = herald_core::claim(GITHUB, &headers, body, now, &herald_core::Inbound::DEFAULT)
            .unwrap();
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
    let public = herald_core::key(ACTIVITYPUB, SPKI).unwrap();
    for (body, holds) in [(body.as_slice(), true), (b"{\"type\":\"Undo\"}", false)] {
        let to = herald_core::Addressed {
            target: "post /inbox",
            audience: "herald.example",
        };
        match herald_core::activity_claim(to, &headers, body, now, &herald_core::Inbound::DEFAULT) {
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
    pollster::block_on(RingSigner::default().public(algorithm, key))
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

#[test]
fn an_rsa_key_is_parsed_once_per_name_and_read_again_by_a_new_signer() {
    let signer = RingSigner::default();
    let key = Secret::new("RSA", pem(&unhex(RSA)));
    let rotated = Secret::new("RSA", pem(&unhex(ROTATED)));
    let first = signer.rsa_pair(&key).unwrap();
    assert!(Arc::ptr_eq(&first, &signer.rsa_pair(&key).unwrap()));
    assert!(Arc::ptr_eq(&first, &signer.rsa_pair(&rotated).unwrap()));
    let published = |signer: &RingSigner, key: &Secret| {
        pollster::block_on(signer.public(Algorithm::RsaSha256, key)).unwrap()
    };
    let fresh = RingSigner::default();
    assert_ne!(published(&fresh, &rotated), published(&signer, &rotated));
    assert_eq!(
        published(&fresh, &rotated),
        subject(
            Public::Rsa,
            RsaKeyPair::from_pkcs8(&unhex(ROTATED))
                .unwrap()
                .public()
                .as_ref()
        )
    );
    assert!(
        fresh
            .rsa_pair(&Secret::new("UNPARSED", "not a key"))
            .is_none()
    );
    assert!(format!("{signer:?}").contains("RSA"));
}

#[test]
fn a_private_key_s_der_is_overwritten_with_zeroes_when_dropped() {
    fn wiped_on_drop<T: zeroize::ZeroizeOnDrop>(_: &T) {}
    let mut der = pkcs8(&Secret::new("RSA", pem(&unhex(RSA)))).unwrap();
    wiped_on_drop(&der);
    assert_eq!(*der, unhex(RSA));
    zeroize::Zeroize::zeroize(&mut der);
    assert!(der.is_empty());
}

#[test]
fn a_sealed_value_opens_only_under_its_key_purpose_and_bound_data_unchanged() {
    let signer = RingSigner::default();
    let key = Secret::new("APP", "the App's key");
    let with = |purpose, bound| Sealing { purpose, bound };
    let seal = |key: &Secret, sealing: Sealing<'_>, plain: &[u8]| {
        pollster::block_on(signer.seal(key, sealing, plain))
    };
    let open = |key: &Secret, sealing: Sealing<'_>, sealed: &[u8]| {
        pollster::block_on(signer.open(key, sealing, sealed))
    };
    let token = with("token", b"bound");
    let sealed = seal(&key, token, b"ghs_minted").unwrap();
    assert_eq!(sealed.len(), 12 + 10 + 16);
    assert_ne!(seal(&key, token, b"ghs_minted").unwrap(), sealed);
    assert_eq!(open(&key, token, &sealed), Ok(b"ghs_minted".to_vec()));
    let refused = Err(CryptoError::Unsealed {
        name: "APP".to_owned(),
    });
    assert_eq!(open(&key, with("other", b"bound"), &sealed), refused);
    assert_eq!(open(&key, with("token", b"other"), &sealed), refused);
    assert_eq!(
        open(&Secret::new("APP", "another key"), token, &sealed),
        refused
    );
    let mut changed = sealed.clone();
    changed[20] ^= 1;
    assert_eq!(open(&key, token, &changed), refused);
    assert_eq!(open(&key, token, &sealed[..8]), refused);
    let web: Vec<u8> = (0..SEALED_ON_THE_WEB.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&SEALED_ON_THE_WEB[at..at + 2], 16).unwrap())
        .collect();
    assert_eq!(open(&key, token, &web), Ok(b"ghs_minted".to_vec()));
}

/// `ghs_minted` sealed by Web Crypto under the key `the App's key` for
/// `token`, bound to `bound`, its nonce 0 to 11.
const SEALED_ON_THE_WEB: &str =
    "000102030405060708090a0b47ec349fd84f0dda1c482d086372fa4ec7df8ddeac7f830d93c1";
