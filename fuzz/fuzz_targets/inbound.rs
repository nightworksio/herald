//! Whatever headers and body a request carries, and wherever it was sent,
//! reading its claim answers a claim or a refusal. A signed claim's message
//! holds the body it was read from, a token's holds nothing, an activity's
//! names its signer and is claimed only with where it was sent, and each
//! claim carries one to as many signatures as `Inbound::DEFAULT` allows,
//! none of them empty.

#![no_main]

use herald_core::{Addressed, Inbound, Scheme, Signed, activity_claim, claim, key};
use jiff::Timestamp;
use libfuzzer_sys::fuzz_target;

const SCHEMES: [Scheme; 7] = [
    herald_sources_forge::Forge::GitHub.scheme(),
    herald_sources_forge::Forge::Forgejo.scheme(),
    herald_sources_forge::Forge::GitLab.scheme(),
    herald_dest_discord::SCHEME,
    herald_dest_slack::SCHEME,
    herald_dest_matrix::SCHEME,
    herald_dest_fediverse::ACTOR_SCHEME,
];

const HEADERS: [&str; 15] = [
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
    "Host",
    "Date",
    "Digest",
    "Signature",
];

fuzz_target!(|input: (Vec<String>, Vec<u8>, i64, String, String)| {
    let (values, body, now, target, audience) = input;
    let Ok(now) = Timestamp::from_second(now) else {
        return;
    };
    let headers: Vec<(&str, &str)> = HEADERS
        .iter()
        .zip(values.iter())
        .map(|(header, value)| (*header, value.as_str()))
        .collect();
    let to = Addressed {
        target: &target,
        audience: &audience,
    };
    if let Ok(claim) = activity_claim(to, &headers, &body, now, &Inbound::DEFAULT) {
        assert!(claim.signer.is_some());
        assert_eq!(claim.signatures.len(), 1);
        assert!(
            claim
                .signatures
                .iter()
                .all(|signature| !signature.is_empty())
        );
    }
    for scheme in SCHEMES {
        if let Ok(claim) = claim(scheme, &headers, &body, now, &Inbound::DEFAULT) {
            assert_ne!(scheme.signed, Signed::HttpSignature);
            if matches!(scheme.signed, Signed::Bearer { .. }) {
                assert!(claim.message.is_empty());
            } else {
                assert!(claim.message.ends_with(&body), "{scheme:?}");
            }
            assert!(
                (1..=Inbound::DEFAULT.signatures).contains(&claim.signatures.len()),
                "{scheme:?}"
            );
            assert!(
                claim
                    .signatures
                    .iter()
                    .all(|signature| !signature.is_empty()),
                "{scheme:?}"
            );
        }
        for value in &values {
            let _ = key(scheme, value);
        }
        let _ = key(
            scheme,
            &format!(
                "-----BEGIN PUBLIC KEY-----{}-----END PUBLIC KEY-----",
                String::from_utf8_lossy(&body)
            ),
        );
    }
});
