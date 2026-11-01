use herald_core::{Refusal, Scheme};
use herald_flows::{Rejected, verified};
use herald_testing::FakeVerifier;
use jiff::Timestamp;

/// A dead signature beside the fake verifier's signature of
/// `msg.1800000000.` and the body, keyed by the bytes 00 01.
const ROTATED: &str = "v1,AAAA v1,SG1hY1NoYTI1NnwAAXxtc2cuMTgwMDAwMDAwMC57InplbiI6IktlZXAgaXQgbG9naWNhbGx5IGF3ZXNvbWUuIn0=";

const BODY: &[u8] = b"{\"zen\":\"Keep it logically awesome.\"}";

fn now() -> Timestamp {
    Timestamp::from_second(1_800_000_000).unwrap()
}

fn hex(bytes: &[u8]) -> String {
    herald_core::hex(bytes)
}

fn checked(scheme: Scheme, key: &str, headers: &[(&str, &str)]) -> Result<(), Rejected> {
    pollster::block_on(verified(&FakeVerifier, scheme, key, headers, BODY, now()))
}

#[test]
fn a_webhook_signed_with_the_shared_secret_is_let_through() {
    let signed = format!(
        "sha256={}",
        hex(&[b"HmacSha256|secret|".as_slice(), BODY].concat())
    );
    assert_eq!(
        checked(
            Scheme::GitHub,
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
        Scheme::GitHub,
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
    assert_eq!(checked(Scheme::Discord, "0aff", &headers), Ok(()));
}

#[test]
fn a_refused_request_is_never_checked_and_says_why() {
    let rejected = checked(Scheme::Slack, "secret", &[]);
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
    assert_eq!(checked(Scheme::GitLab, "whsec_AAE=", &headers), Ok(()));
    let only_bad = [
        ("webhook-id", "msg"),
        ("webhook-timestamp", "1800000000"),
        ("webhook-signature", "v1,AAAA"),
    ];
    assert_eq!(
        checked(Scheme::GitLab, "whsec_AAE=", &only_bad),
        Err(Rejected::Forged)
    );
}

#[test]
fn a_key_not_written_in_its_form_lets_nothing_through() {
    let rejected = checked(Scheme::GitLab, "AAE=", &[]);
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
            Scheme::Matrix,
            "hs-token",
            &[("Authorization", "Bearer hs-token")]
        ),
        Ok(())
    );
    assert_eq!(
        checked(
            Scheme::Matrix,
            "hs-token",
            &[("Authorization", "Bearer as-token")]
        ),
        Err(Rejected::Forged)
    );
}
