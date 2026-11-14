use std::fmt::Write as _;

use herald_core::{Refusal, Scheme};
use herald_flows::{
    Admission, Checking, Keys, Rejected, Verified, activity_verified, admitted, released, verified,
};
use herald_ports::{Keyring, KnownKey, StoreError};
use herald_records::MemoryKeyring;
use herald_testing::{FakeStore, FakeVerifier};
use jiff::Timestamp;

use crate::harness::{Refusing, keys_in};

/// A dead signature beside the fake verifier's signature of
/// `msg.1800000000.` and the body, keyed by the bytes 00 01.
const ROTATED: &str = "v1,AAAA v1,SG1hY1NoYTI1NnwAAXxtc2cuMTgwMDAwMDAwMC57InplbiI6IktlZXAgaXQgbG9naWNhbGx5IGF3ZXNvbWUuIn0=";

const BODY: &[u8] = b"{\"zen\":\"Keep it logically awesome.\"}";

fn now() -> Timestamp {
    Timestamp::from_second(1_800_000_000).unwrap()
}

fn checking(now: Timestamp) -> Checking<'static> {
    Checking {
        now,
        bounds: &herald_core::Inbound::DEFAULT,
    }
}

fn hex(bytes: &[u8]) -> String {
    herald_core::hex(bytes)
}

fn checked(scheme: Scheme, key: &str, headers: &[(&str, &str)]) -> Result<(), Rejected> {
    pollster::block_on(verified(
        &FakeVerifier,
        scheme,
        key,
        headers,
        BODY,
        checking(now()),
    ))
    .map(|_| ())
}

#[test]
fn a_webhook_signed_with_the_shared_secret_is_let_through() {
    let signed = format!(
        "sha256={}",
        hex(&[b"HmacSha256|secret|".as_slice(), BODY].concat())
    );
    assert_eq!(
        checked(
            herald_sources_forge::Forge::GitHub.scheme(),
            "secret",
            &[("X-Hub-Signature-256", &signed)]
        ),
        Ok(())
    );
}

#[test]
fn a_webhook_signed_with_another_secret_is_forged() {
    let signed = format!(
        "sha256={}",
        hex(&[b"HmacSha256|other|".as_slice(), BODY].concat())
    );
    let rejected = checked(
        herald_sources_forge::Forge::GitHub.scheme(),
        "secret",
        &[("X-Hub-Signature-256", &signed)],
    );
    assert_eq!(rejected, Err(Rejected::Forged));
    assert_eq!(
        rejected.unwrap_err().to_string(),
        "the request's signature is not its sender's"
    );
}

#[test]
fn a_discord_interaction_is_checked_as_ed25519_over_its_timestamp_and_body() {
    let message = [b"1800000000".as_slice(), BODY].concat();
    let signed = hex(&[b"Ed25519|\x0a\xff|".as_slice(), &message].concat());
    let headers = [
        ("X-Signature-Ed25519", signed.as_str()),
        ("X-Signature-Timestamp", "1800000000"),
    ];
    assert_eq!(
        checked(herald_dest_discord::SCHEME, "0aff", &headers),
        Ok(())
    );
}

#[test]
fn a_discord_interaction_delivered_twice_is_a_replay() {
    let message = [b"1800000000".as_slice(), BODY].concat();
    let signed = hex(&[b"Ed25519|\x0a\xff|".as_slice(), &message].concat());
    let headers = [
        ("X-Signature-Ed25519", signed.as_str()),
        ("X-Signature-Timestamp", "1800000000"),
    ];
    let store = FakeStore::default();
    let delivery = || {
        pollster::block_on(async {
            let verified = verified(
                &FakeVerifier,
                herald_dest_discord::SCHEME,
                "0aff",
                &headers,
                BODY,
                checking(now()),
            )
            .await?;
            admitted(
                &store,
                Admission::Once,
                herald_dest_discord::SCHEME.name,
                &verified,
                now(),
            )
            .await
        })
    };
    assert_eq!(delivery(), Ok(()));
    assert_eq!(delivery(), Err(Rejected::Replayed));
}

#[test]
fn a_refused_request_is_never_checked_and_says_why() {
    let rejected = checked(herald_dest_slack::SCHEME, "secret", &[]);
    assert_eq!(
        rejected,
        Err(Rejected::Refused(Refusal::Unsigned {
            header: "X-Slack-Request-Timestamp"
        }))
    );
    assert_eq!(
        rejected.unwrap_err().to_string(),
        "the request carries no X-Slack-Request-Timestamp"
    );
}

#[test]
fn a_gitlab_webhook_holds_where_any_one_of_its_signatures_does() {
    let headers = [
        ("webhook-id", "msg"),
        ("webhook-timestamp", "1800000000"),
        ("webhook-signature", ROTATED),
    ];
    assert_eq!(
        checked(
            herald_sources_forge::Forge::GitLab.scheme(),
            "whsec_AAE=",
            &headers
        ),
        Ok(())
    );
    let only_bad = [
        ("webhook-id", "msg"),
        ("webhook-timestamp", "1800000000"),
        ("webhook-signature", "v1,AAAA"),
    ];
    assert_eq!(
        checked(
            herald_sources_forge::Forge::GitLab.scheme(),
            "whsec_AAE=",
            &only_bad
        ),
        Err(Rejected::Forged)
    );
}

#[test]
fn a_key_not_written_in_its_form_lets_nothing_through() {
    let rejected = checked(herald_sources_forge::Forge::GitLab.scheme(), "AAE=", &[]);
    assert_eq!(rejected, Err(Rejected::Unkeyed));
    assert_eq!(
        rejected.unwrap_err().to_string(),
        "the key the request is checked with is not written in its form, so no request is let through"
    );
}

#[test]
fn a_matrix_transaction_holds_only_with_the_homeserver_token() {
    assert_eq!(
        checked(
            herald_dest_matrix::SCHEME,
            "hs-token",
            &[("Authorization", "Bearer hs-token")]
        ),
        Ok(())
    );
    assert_eq!(
        checked(
            herald_dest_matrix::SCHEME,
            "hs-token",
            &[("Authorization", "Bearer as-token")]
        ),
        Err(Rejected::Forged)
    );
}

/// What a follow signed with Ana's key is let through as.
fn followed() -> Verified {
    Verified {
        delivery: Some(signed(FOLLOW).iter().fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })),
        signer: Some(ANA.to_owned()),
    }
}

fn delivered(id: &str) -> Verified {
    Verified {
        delivery: Some(id.to_owned()),
        signer: None,
    }
}

#[test]
fn a_github_webhook_is_delivered_as_its_signature() {
    let signed = format!(
        "sha256={}",
        hex(&[b"HmacSha256|secret|".as_slice(), BODY].concat())
    );
    let verified = pollster::block_on(verified(
        &FakeVerifier,
        herald_sources_forge::Forge::GitHub.scheme(),
        "secret",
        &[("X-Hub-Signature-256", &signed)],
        BODY,
        checking(now()),
    ))
    .unwrap();
    assert_eq!(verified, delivered(&signed["sha256=".len()..]));
}

#[test]
fn a_delivery_is_admitted_once_until_it_is_released() {
    let store = FakeStore::default();
    let admit = |verified: &Verified| {
        pollster::block_on(admitted(
            &store,
            Admission::Once,
            herald_sources_forge::Forge::GitHub.scheme().name,
            verified,
            now(),
        ))
    };
    assert_eq!(admit(&delivered("1")), Ok(()));
    assert_eq!(admit(&delivered("1")), Err(Rejected::Replayed));
    assert_eq!(
        pollster::block_on(admitted(
            &store,
            Admission::Again,
            herald_sources_forge::Forge::GitHub.scheme().name,
            &delivered("1"),
            now()
        )),
        Ok(()),
        "a request handed in again is let through"
    );
    assert_eq!(Admission::default(), Admission::Once);
    assert_eq!(
        Rejected::Replayed.to_string(),
        "the request was delivered before, so it is taken for a replay"
    );
    assert_eq!(
        pollster::block_on(released(
            &store,
            herald_sources_forge::Forge::GitHub.scheme().name,
            &delivered("1")
        )),
        Ok(())
    );
    assert_eq!(admit(&delivered("1")), Ok(()));
    let unnamed = Verified {
        delivery: None,
        signer: None,
    };
    assert_eq!(admit(&unnamed), Ok(()));
    assert_eq!(admit(&unnamed), Ok(()));
    assert_eq!(
        pollster::block_on(released(
            &store,
            herald_sources_forge::Forge::GitHub.scheme().name,
            &unnamed
        )),
        Ok(())
    );
}

#[test]
fn a_store_keeping_no_deliveries_admits_every_one_and_the_ledger_skips_replays() {
    let store = Refusing(StoreError::Unsupported { what: "deliveries" });
    let verified = delivered("1");
    assert_eq!(
        pollster::block_on(admitted(
            &store,
            Admission::Once,
            herald_sources_forge::Forge::GitHub.scheme().name,
            &verified,
            now()
        )),
        Ok(())
    );
    assert_eq!(
        pollster::block_on(released(
            &store,
            herald_sources_forge::Forge::GitHub.scheme().name,
            &verified
        )),
        Ok(())
    );
}

#[test]
fn a_store_that_cannot_answer_lets_nothing_through() {
    let down = StoreError::Unavailable {
        reason: "down".to_owned(),
    };
    let store = Refusing(down.clone());
    let verified = delivered("1");
    let rejected = pollster::block_on(admitted(
        &store,
        Admission::Once,
        herald_sources_forge::Forge::GitHub.scheme().name,
        &verified,
        now(),
    ));
    assert_eq!(
        rejected,
        Err(Rejected::Unadmitted {
            reason: down.to_string()
        })
    );
    assert_eq!(
        rejected.unwrap_err().to_string(),
        "the store could not say whether the request was delivered before, so it is not let through: the store could not be reached: down"
    );
    assert_eq!(
        pollster::block_on(released(
            &store,
            herald_sources_forge::Forge::GitHub.scheme().name,
            &verified
        )),
        Err(down)
    );
}

pub(crate) const ACTOR_KEY: &str =
    include_str!("../../../herald-crypto/fixtures/rsa-2048-spki.test.pem");

pub(crate) const ANA: &str = "https://social.example/users/ana";
pub(crate) const ANA_KEY: &str = "https://social.example/users/ana#main-key";
pub(crate) const FOLLOW: &[u8] = br#"{"type":"Follow","actor":"https://social.example/users/ana"}"#;
/// Where the activities here are sent: herald's inbox, on its own host.
const TO: herald_core::Addressed<'static> = herald_core::Addressed {
    target: "post /inbox",
    audience: "herald.example",
};

pub(crate) fn at() -> Timestamp {
    "2027-01-15T08:00:00Z".parse().unwrap()
}

/// The headers of `body` signed with `key_id` by the fake verifier's
/// signature under the actor key, or with `signature` where it is given.
pub(crate) fn activity(
    body: &[u8],
    key_id: &str,
    signature: Option<&str>,
) -> Vec<(String, String)> {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use sha2::Digest as _;
    let digest = format!("SHA-256={}", STANDARD.encode(sha2::Sha256::digest(body)));
    let signing = |signature: &str| {
        vec![
            ("Host".to_owned(), "herald.example".to_owned()),
            (
                "Date".to_owned(),
                "Fri, 15 Jan 2027 08:00:00 GMT".to_owned(),
            ),
            ("Digest".to_owned(), digest.clone()),
            (
                "Signature".to_owned(),
                format!(
                    r#"keyId="{key_id}",headers="(request-target) host date digest",signature="{signature}""#
                ),
            ),
        ]
    };
    match signature {
        Some(signature) => signing(signature),
        None => signing(&STANDARD.encode(signed(body))),
    }
}

/// What Ana's key signs `body`'s activity as, as [`FakeSigner`] spells it.
fn signed(body: &[u8]) -> Vec<u8> {
    let probe = activity(body, ANA_KEY, Some("AA=="));
    let message = herald_core::activity_claim(
        TO,
        &borrowed(&probe),
        body,
        at(),
        &herald_core::Inbound::DEFAULT,
    )
    .unwrap()
    .message;
    let public = herald_core::key(herald_dest_fediverse::ACTOR_SCHEME, ACTOR_KEY).unwrap();
    [b"RsaSha256|".as_slice(), &public, b"|", &message].concat()
}

pub(crate) fn borrowed(headers: &[(String, String)]) -> Vec<(&str, &str)> {
    headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect()
}

pub(crate) fn document(owner: &str) -> String {
    serde_json::json!({
        "id": ANA,
        "type": "Person",
        "publicKey": { "id": ANA_KEY, "owner": owner, "publicKeyPem": ACTOR_KEY },
    })
    .to_string()
}

/// A scripted answer: the URL's end, its status and its body.
type Answer<'a> = (&'a str, u16, &'a str);

fn checked_activity(
    body: &[u8],
    headers: &[(String, String)],
    answers: &[Answer],
) -> (Result<Verified, Rejected>, Vec<String>) {
    served(body, headers, answers, "application/activity+json")
}

fn served(
    body: &[u8],
    headers: &[(String, String)],
    answers: &[Answer],
    content_type: &str,
) -> (Result<Verified, Rejected>, Vec<String>) {
    kept_by(&MemoryKeyring::new(8), body, headers, answers, content_type)
}

/// How long a key is used before it is read again, in these tests.
const REFRESHED: jiff::SignedDuration = jiff::SignedDuration::from_hours(1);
const REREAD: jiff::SignedDuration = jiff::SignedDuration::from_mins(1);

fn kept_by<K: Keyring>(
    keyring: &K,
    body: &[u8],
    headers: &[(String, String)],
    answers: &[Answer],
    content_type: &str,
) -> (Result<Verified, Rejected>, Vec<String>) {
    let script =
        crate::harness::Script::answering(answers).headed(&[("content-type", content_type)]);
    let keys = Keys {
        keyring,
        refreshed: REFRESHED,
        reread: REREAD,
    };
    let checked = pollster::block_on(activity_verified(
        &FakeVerifier,
        &script,
        &keys,
        TO,
        &borrowed(headers),
        body,
        checking(at()),
    ));
    (checked, script.seen())
}

const JSON: &str = "application/activity+json";

fn known(owner: &str, fetched: Timestamp) -> KnownKey {
    KnownKey {
        owner: owner.to_owned(),
        pem: ACTOR_KEY.to_owned(),
        fetched,
    }
}

#[test]
fn a_key_read_once_checks_the_next_activity_without_reading_it_again() {
    let keyring = MemoryKeyring::new(8);
    let owned = document(ANA);
    let headers = activity(FOLLOW, ANA_KEY, None);
    let (first, seen) = kept_by(
        &keyring,
        FOLLOW,
        &headers,
        &[(ANA, 200, owned.as_str())],
        JSON,
    );
    assert_eq!(first, Ok(followed()));
    assert_eq!(seen, [format!("GET {ANA}")]);
    assert_eq!(
        pollster::block_on(keyring.known(ANA_KEY)).unwrap(),
        Some(known(ANA, at()))
    );
    let (again, seen) = kept_by(&keyring, FOLLOW, &headers, &[], JSON);
    assert_eq!(again.unwrap().signer.as_deref(), Some(ANA));
    assert_eq!(seen, [] as [String; 0]);
}

#[test]
fn a_key_s_document_is_read_no_further_than_the_bounds_let_it() {
    let owned = document(ANA);
    let headers = activity(FOLLOW, ANA_KEY, None);
    let script = crate::harness::Script::answering(&[(ANA, 200, owned.as_str())])
        .headed(&[("content-type", JSON)]);
    let keyring = MemoryKeyring::new(8);
    let keys = Keys {
        keyring: &keyring,
        refreshed: REFRESHED,
        reread: REREAD,
    };
    let bounds = herald_core::Inbound {
        document: owned.len() - 1,
        ..herald_core::Inbound::DEFAULT
    };
    let checked = pollster::block_on(activity_verified(
        &FakeVerifier,
        &script,
        &keys,
        TO,
        &borrowed(&headers),
        FOLLOW,
        Checking {
            now: at(),
            bounds: &bounds,
        },
    ));
    let Err(Rejected::Unfetched { reason }) = checked else {
        panic!("{checked:?}");
    };
    assert_eq!(
        reason,
        format!(
            "the host could not be reached: the response is larger than {} bytes",
            owned.len() - 1
        )
    );
}

#[test]
fn a_key_kept_as_long_as_its_refresh_is_read_again() {
    let keyring = MemoryKeyring::new(8);
    let stale = at().checked_sub(REFRESHED).unwrap();
    pollster::block_on(keyring.know(ANA_KEY, &known(ANA, stale))).unwrap();
    let owned = document(ANA);
    let headers = activity(FOLLOW, ANA_KEY, None);
    let (checked, seen) = kept_by(
        &keyring,
        FOLLOW,
        &headers,
        &[(ANA, 200, owned.as_str())],
        JSON,
    );
    assert_eq!(checked, Ok(followed()));
    assert_eq!(seen, [format!("GET {ANA}")]);
    assert_eq!(
        pollster::block_on(keyring.known(ANA_KEY)).unwrap(),
        Some(known(ANA, at()))
    );
}

#[test]
fn a_key_kept_just_inside_its_refresh_is_used() {
    let keyring = MemoryKeyring::new(8);
    let fresh = at()
        .checked_sub(REFRESHED)
        .unwrap()
        .checked_add(jiff::SignedDuration::from_secs(1))
        .unwrap();
    pollster::block_on(keyring.know(ANA_KEY, &known(ANA, fresh))).unwrap();
    let headers = activity(FOLLOW, ANA_KEY, None);
    let (checked, seen) = kept_by(&keyring, FOLLOW, &headers, &[], JSON);
    assert_eq!(checked, Ok(followed()));
    assert_eq!(seen, [] as [String; 0]);
}

#[test]
fn a_kept_key_the_activity_does_not_hold_under_is_read_again_once_its_reread_has_passed() {
    let keyring = MemoryKeyring::new(8);
    let rotated = "https://social.example/users/bob";
    let read = at().checked_sub(REREAD).unwrap();
    pollster::block_on(keyring.know(ANA_KEY, &known(rotated, read))).unwrap();
    let owned = document(ANA);
    let headers = activity(FOLLOW, ANA_KEY, None);
    let (checked, seen) = kept_by(
        &keyring,
        FOLLOW,
        &headers,
        &[(ANA, 200, owned.as_str())],
        JSON,
    );
    assert_eq!(checked.unwrap().signer.as_deref(), Some(ANA));
    assert_eq!(seen, [format!("GET {ANA}")]);
}

#[test]
fn an_activity_that_does_not_hold_under_a_key_read_within_its_reread_is_refused_unread() {
    let keyring = MemoryKeyring::new(8);
    let rotated = "https://social.example/users/bob";
    let read = at()
        .checked_sub(REREAD)
        .unwrap()
        .checked_add(jiff::SignedDuration::from_secs(1))
        .unwrap();
    pollster::block_on(keyring.know(ANA_KEY, &known(rotated, read))).unwrap();
    let owned = document(ANA);
    let headers = activity(FOLLOW, ANA_KEY, None);
    let (checked, seen) = kept_by(
        &keyring,
        FOLLOW,
        &headers,
        &[(ANA, 200, owned.as_str())],
        JSON,
    );
    assert_eq!(checked, Err(Rejected::Impersonated));
    assert_eq!(seen, [] as [String; 0]);
}

/// A keyring answering each call as it is set to.
pub(crate) struct Answering {
    pub(crate) known: Result<Option<KnownKey>, StoreError>,
    pub(crate) know: Result<(), StoreError>,
    pub(crate) forget: Result<(), StoreError>,
}

impl Keyring for Answering {
    async fn known(&self, _: &str) -> Result<Option<KnownKey>, StoreError> {
        self.known.clone()
    }

    async fn know(&self, _: &str, _: &KnownKey) -> Result<(), StoreError> {
        self.know.clone()
    }

    async fn forget_before(&self, _: Timestamp) -> Result<(), StoreError> {
        self.forget.clone()
    }
}

fn down() -> StoreError {
    StoreError::Unavailable {
        reason: "down".to_owned(),
    }
}

pub(crate) fn keepless() -> Answering {
    let unsupported = || StoreError::Unsupported { what: "keys" };
    Answering {
        known: Err(unsupported()),
        know: Err(unsupported()),
        forget: Err(unsupported()),
    }
}

#[test]
fn a_keyring_keeping_no_keys_reads_every_key_from_its_document() {
    let owned = document(ANA);
    let headers = activity(FOLLOW, ANA_KEY, None);
    let keyring = keepless();
    let (checked, seen) = kept_by(
        &keyring,
        FOLLOW,
        &headers,
        &[(ANA, 200, owned.as_str())],
        JSON,
    );
    assert_eq!(checked, Ok(followed()));
    assert_eq!(seen, [format!("GET {ANA}")]);
    assert_eq!(pollster::block_on(keys_in(&keyring).sweep(at())), Ok(()));
}

#[test]
fn a_keyring_that_cannot_answer_lets_no_activity_through_and_says_why() {
    let owned = document(ANA);
    let headers = activity(FOLLOW, ANA_KEY, None);
    let unread = Answering {
        known: Err(down()),
        ..keepless()
    };
    let (checked, seen) = kept_by(&unread, FOLLOW, &headers, &[], JSON);
    let rejected = Rejected::Unkept {
        reason: down().to_string(),
    };
    assert_eq!(checked, Err(rejected.clone()));
    assert_eq!(seen, [] as [String; 0]);
    assert_eq!(
        rejected.to_string(),
        format!(
            "the keys activities are checked with could not be read or kept, so the activity is not let through: {}",
            down()
        )
    );
    let unkept = Answering {
        know: Err(down()),
        ..keepless()
    };
    let (checked, seen) = kept_by(
        &unkept,
        FOLLOW,
        &headers,
        &[(ANA, 200, owned.as_str())],
        JSON,
    );
    assert_eq!(checked, Err(rejected));
    assert_eq!(seen, [format!("GET {ANA}")]);
    let unswept = Answering {
        forget: Err(down()),
        ..keepless()
    };
    assert_eq!(
        pollster::block_on(keys_in(&unswept).sweep(at())),
        Err(down())
    );
}

#[test]
fn an_activity_is_checked_with_its_actor_s_key_and_names_that_actor() {
    let owned = document(ANA);
    let (checked, seen) = checked_activity(
        FOLLOW,
        &activity(FOLLOW, ANA_KEY, None),
        &[(ANA, 200, owned.as_str())],
    );
    assert_eq!(checked, Ok(followed()));
    assert_eq!(seen, [format!("GET {ANA}")]);
}

#[test]
fn an_activity_whose_actor_is_not_its_signer_is_impersonated() {
    let owned = document(ANA);
    let other = br#"{"type":"Follow","actor":"https://social.example/users/bob"}"#;
    let (checked, _) = checked_activity(
        other,
        &activity(other, ANA_KEY, None),
        &[(ANA, 200, owned.as_str())],
    );
    assert_eq!(checked, Err(Rejected::Impersonated));
    assert_eq!(
        Rejected::Impersonated.to_string(),
        "the activity names an actor other than the owner of the key that signed it, or has its id on another origin"
    );
}

#[test]
fn an_activity_with_a_forged_signature_is_forged_before_its_actor_is_read() {
    let owned = document(ANA);
    let (checked, _) = checked_activity(
        FOLLOW,
        &activity(FOLLOW, ANA_KEY, Some("AAE=")),
        &[(ANA, 200, owned.as_str())],
    );
    assert_eq!(checked, Err(Rejected::Forged));
}

#[test]
fn a_key_that_cannot_be_read_lets_no_activity_through_and_says_why() {
    let owned = document(ANA);
    let elsewhere = document("https://victim.example/users/admin");
    let unkeyed = serde_json::json!({
        "id": ANA,
        "type": "Person",
        "publicKey": { "id": ANA_KEY, "owner": ANA, "publicKeyPem": "not a key" },
    })
    .to_string();
    let cases: [(&str, Vec<Answer>, &str); 4] = [
        (ANA_KEY, vec![(ANA, 410, "")], "its server answered 410"),
        (
            ANA_KEY,
            vec![],
            "the host could not be reached: nothing scripted for GET https://social.example/users/ana",
        ),
        (
            ANA_KEY,
            vec![(ANA, 200, elsewhere.as_str())],
            "the document is not the one it was read from, or names another actor as the key's owner",
        ),
        (
            "http://social.example/users/ana#main-key",
            vec![(ANA, 200, owned.as_str())],
            "http://social.example/users/ana#main-key is not an HTTPS URL naming a host",
        ),
    ];
    for (key_id, answers, reason) in cases {
        let (checked, _) =
            checked_activity(FOLLOW, &activity(FOLLOW, key_id, Some("AAE=")), &answers);
        let Err(Rejected::Unfetched { reason: said }) = checked else {
            panic!("{key_id}: {checked:?}");
        };
        assert_eq!(said, reason);
        assert_eq!(
            Rejected::Unfetched {
                reason: said.clone()
            }
            .to_string(),
            format!("the key the activity is signed with could not be read: {said}")
        );
    }
    let (checked, _) = checked_activity(
        FOLLOW,
        &activity(FOLLOW, ANA_KEY, Some("AAE=")),
        &[(ANA, 200, unkeyed.as_str())],
    );
    assert_eq!(checked, Err(Rejected::Unkeyed));
    let (refused, seen) = checked_activity(FOLLOW, &[], &[]);
    assert_eq!(
        refused,
        Err(Rejected::Refused(Refusal::Unsigned {
            header: "Signature"
        }))
    );
    assert_eq!(seen, Vec::<String>::new());
}

#[test]
fn a_key_served_as_anything_but_activitypub_json_is_not_read() {
    let owned = document(ANA);
    for content_type in ["text/plain", "application/json"] {
        let (checked, _) = served(
            FOLLOW,
            &activity(FOLLOW, ANA_KEY, None),
            &[(ANA, 200, owned.as_str())],
            content_type,
        );
        assert_eq!(
            checked,
            Err(Rejected::Unfetched {
                reason: "its server did not answer it as ActivityPub JSON".to_owned()
            }),
            "{content_type}"
        );
    }
}

#[test]
fn an_activity_is_never_checked_against_a_key_handed_in() {
    let headers = activity(FOLLOW, ANA_KEY, None);
    let handed = pollster::block_on(verified(
        &FakeVerifier,
        herald_dest_fediverse::ACTOR_SCHEME,
        ACTOR_KEY,
        &borrowed(&headers),
        FOLLOW,
        checking(at()),
    ));
    assert_eq!(handed, Err(Rejected::Unkeyed));
}

#[test]
fn a_key_s_own_document_is_its_owner_s_only_where_the_owner_lists_it() {
    let alone = "https://social.example/users/ana/main-key";
    let key =
        serde_json::json!({ "id": alone, "owner": ANA, "publicKeyPem": ACTOR_KEY }).to_string();
    let listing = serde_json::json!({ "id": ANA, "type": "Person", "publicKey": { "id": alone, "owner": ANA } })
        .to_string();
    let elsewhere =
        serde_json::json!({ "id": ANA, "type": "Person", "publicKey": { "id": ANA_KEY } })
            .to_string();
    let (checked, seen) = checked_activity(
        FOLLOW,
        &activity(FOLLOW, alone, None),
        &[(alone, 200, key.as_str()), (ANA, 200, listing.as_str())],
    );
    assert_eq!(
        checked.map(|verified| verified.signer),
        Ok(Some(ANA.to_owned()))
    );
    assert_eq!(seen, [format!("GET {alone}"), format!("GET {ANA}")]);
    let (unlisted, _) = checked_activity(
        FOLLOW,
        &activity(FOLLOW, alone, None),
        &[(alone, 200, key.as_str()), (ANA, 200, elsewhere.as_str())],
    );
    assert_eq!(
        unlisted,
        Err(Rejected::Unfetched {
            reason: "the actor the key names as its owner does not list the key".to_owned()
        })
    );
    let (gone, _) = checked_activity(
        FOLLOW,
        &activity(FOLLOW, alone, None),
        &[(alone, 200, key.as_str()), (ANA, 404, "")],
    );
    assert_eq!(
        gone,
        Err(Rejected::Unfetched {
            reason: "its server answered 404".to_owned()
        })
    );
}

#[test]
fn an_activity_whose_own_id_is_on_another_origin_than_its_actor_is_impersonated() {
    let owned = document(ANA);
    let foreign = br#"{"type":"Follow","id":"https://elsewhere.example/f/1","actor":"https://social.example/users/ana"}"#;
    let (checked, _) = checked_activity(
        foreign,
        &activity(foreign, ANA_KEY, None),
        &[(ANA, 200, owned.as_str())],
    );
    assert_eq!(checked, Err(Rejected::Impersonated));
    let homed = br#"{"type":"Follow","id":"https://social.example/f/1","actor":"https://social.example/users/ana"}"#;
    let (checked, _) = checked_activity(
        homed,
        &activity(homed, ANA_KEY, None),
        &[(ANA, 200, owned.as_str())],
    );
    assert_eq!(
        checked.map(|verified| verified.signer),
        Ok(Some(ANA.to_owned()))
    );
    let actorless = br#"{"type":"Follow"}"#;
    let (checked, _) = checked_activity(
        actorless,
        &activity(actorless, ANA_KEY, None),
        &[(ANA, 200, owned.as_str())],
    );
    assert_eq!(checked, Err(Rejected::Impersonated));
}
