//! Whatever headers and body a request carries, reading its claim answers a
//! claim or a refusal, and a claim's message holds the body it was read from.

#![no_main]

use herald_core::{Scheme, claim};
use jiff::Timestamp;
use libfuzzer_sys::fuzz_target;

const SCHEMES: [Scheme; 4] = [Scheme::GitHub, Scheme::Forgejo, Scheme::Discord, Scheme::Slack];

const HEADERS: [&str; 7] = [
    "X-Hub-Signature-256",
    "X-Forgejo-Signature",
    "X-Gitea-Signature",
    "X-Signature-Ed25519",
    "X-Signature-Timestamp",
    "X-Slack-Signature",
    "X-Slack-Request-Timestamp",
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
            assert!(claim.message.ends_with(&body), "{scheme:?}");
        }
    }
});
