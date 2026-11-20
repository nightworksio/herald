//! Whatever text a request presents as an ID token, and whatever an issuer
//! answers for its discovery document and key set, reading them answers a
//! token, keys or a refusal, never a panic. A refusal never quotes the
//! token, a token held to its times names them both, what it is
//! remembered by never quotes it, a discovery document only names keys
//! https on the issuer's own origin, and a claim pattern never matches
//! across a `/` its claim has and the pattern has not.

#![no_main]

use std::collections::BTreeMap;

use herald_core::{ClaimPattern, IdToken, KeySet, TokenAlgorithm, Untaken, jwks_uri, same_origin};
use jiff::{SignedDuration, Timestamp};
use libfuzzer_sys::fuzz_target;

const ISSUER: &str = "https://token.actions.githubusercontent.com";
const AUDIENCE: &str = "https://herald.example";

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let mut parts = text.splitn(3, '\u{0}');
    let token = parts.next().unwrap_or_default();
    let pattern = parts.next().unwrap_or_default();
    let answer = parts.next().unwrap_or_default();
    let now = Timestamp::from_second(1_800_000_000).unwrap_or(Timestamp::UNIX_EPOCH);
    match IdToken::read(token) {
        Ok(read) => {
            assert!(IdToken::shaped(token));
            let checked = read.algorithm(&TokenAlgorithm::ALL).and_then(|_| {
                read.held_to(
                    AUDIENCE,
                    now,
                    (SignedDuration::from_secs(60), SignedDuration::from_hours(1)),
                )
            });
            if checked.is_ok() {
                assert!(
                    read.valid()
                        .is_some_and(|(issued, expires)| issued <= expires)
                );
            }
            assert!(!read.remembered_as().contains(token) || token.len() < 8);
            let wanted = BTreeMap::from([("sub".to_owned(), ClaimPattern::new(pattern))]);
            for refused in [checked.err(), read.matching(&wanted).err()]
                .into_iter()
                .flatten()
            {
                let said = refused.to_string();
                assert!(token.len() < 8 || !said.contains(token), "{said}");
            }
            let _ = read.evidence(&wanted);
            let _ = format!("{read:?}");
        }
        Err(refused) => assert_eq!(refused, Untaken::Unreadable),
    }
    let claim = answer.lines().next().unwrap_or_default();
    if ClaimPattern::new(pattern).matches(claim) {
        assert_eq!(pattern.matches('/').count(), claim.matches('/').count());
    }
    if let Ok(keys) = KeySet::read(answer.as_bytes()) {
        let _ = keys.key(None);
    }
    if let Ok(named) = jwks_uri(answer.as_bytes(), ISSUER) {
        assert!(same_origin(&named, ISSUER), "{named}");
    }
});
