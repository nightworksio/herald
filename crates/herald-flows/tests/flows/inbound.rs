use herald_core::{Refusal, Scheme};
use herald_flows::{Rejected, verified};
use herald_testing::FakeVerifier;
use jiff::Timestamp;

const BODY: &[u8] = b"{\"zen\":\"Keep it logically awesome.\"}";

fn now() -> Timestamp {
    Timestamp::from_second(1_800_000_000).unwrap()
}

fn hex(bytes: &[u8]) -> String {
    herald_core::hex(bytes)
}

fn checked(scheme: Scheme, key: &[u8], headers: &[(&str, &str)]) -> Result<(), Rejected> {
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
            b"secret",
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
        b"secret",
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
    let signed = hex(&[b"Ed25519|public|".as_slice(), &message].concat());
    let headers = [
        ("X-Signature-Ed25519", signed.as_str()),
        ("X-Signature-Timestamp", "1800000000"),
    ];
    assert_eq!(checked(Scheme::Discord, b"public", &headers), Ok(()));
}

#[test]
fn a_refused_request_is_never_checked_and_says_why() {
    let rejected = checked(Scheme::Slack, b"secret", &[]);
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
