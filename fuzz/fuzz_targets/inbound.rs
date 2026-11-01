//! Whatever headers and body a request carries, reading its claim answers a
//! claim or a refusal. A signed claim's message holds the body it was read
//! from and a token's holds nothing, and each claim carries one to
//! `MOST_SIGNATURES` signatures, none of them empty.

#![no_main]

use herald_core::{Scheme, claim, key};
use jiff::Timestamp;
use libfuzzer_sys::fuzz_target;

const SCHEMES: [Scheme; 6] = [
    Scheme::GitHub,
    Scheme::Forgejo,
    Scheme::GitLab,
    Scheme::Discord,
    Scheme::Slack,
    Scheme::Matrix,
];

const HEADERS: [&str; 11] = [
    "X-Hub-Signature-256",
    "X-Forgejo-Signature",
    "X-Gitea-Signature",
    "X-Signature-Ed25519",
    "X-Signature-Timestamp",
    "X-Slack-Signature",
    "X-Slack-Request-Timestamp",
    "webhook-id",
    "webhook-timestamp",
    "webhook-signature",
    "Authorization",
];

fuzz_target!(|input: (Vec<String>, Vec<u8>, i64)| {
    let (values, body, now) = input;
    let Ok(now) = Timestamp::from_second(now) else {
        return;
    };
    let headers: Vec<(&str, &str)> = HEADERS
        .iter()
        .zip(values.iter())
        .map(|(header, value)| (*header, value.as_str()))
        .collect();
    for scheme in SCHEMES {
        if let Ok(claim) = claim(scheme, &headers, &body, now) {
            if scheme == Scheme::Matrix {
                assert!(claim.message.is_empty());
            } else {
                assert!(claim.message.ends_with(&body), "{scheme:?}");
            }
            assert!((1..=herald_core::MOST_SIGNATURES).contains(&claim.signatures.len()), "{scheme:?}");
            assert!(claim.signatures.iter().all(|signature| !signature.is_empty()), "{scheme:?}");
        }
        for value in &values {
            let _ = key(scheme, value);
        }
    }
});
