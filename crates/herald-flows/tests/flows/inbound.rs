use herald_core::{Refusal, Scheme};
use herald_flows::{Rejected, Verified, admitted, released, verified};
use herald_ports::{Store, StoreError};
use herald_testing::{FakeStore, FakeVerifier};
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
    pollster::block_on(verified(&FakeVerifier, scheme, key, headers, BODY, now())).map(|_| ())
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

fn delivered(id: &str) -> Verified {
    Verified {
        delivery: Some(id.to_owned()),
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
        Scheme::GitHub,
        "secret",
        &[("X-Hub-Signature-256", &signed)],
        BODY,
        now(),
    ))
    .unwrap();
    assert_eq!(verified, delivered(&signed["sha256=".len()..]));
}

#[test]
fn a_delivery_is_admitted_once_until_it_is_released() {
    let store = FakeStore::default();
    let admit =
        |verified: &Verified| pollster::block_on(admitted(&store, Scheme::GitHub, verified, now()));
    assert_eq!(admit(&delivered("1")), Ok(()));
    assert_eq!(admit(&delivered("1")), Err(Rejected::Replayed));
    assert_eq!(
        Rejected::Replayed.to_string(),
        "the request was delivered before, so it is taken for a replay"
    );
    assert_eq!(
        pollster::block_on(released(&store, Scheme::GitHub, &delivered("1"))),
        Ok(())
    );
    assert_eq!(admit(&delivered("1")), Ok(()));
    let unnamed = Verified { delivery: None };
    assert_eq!(admit(&unnamed), Ok(()));
    assert_eq!(admit(&unnamed), Ok(()));
    assert_eq!(
        pollster::block_on(released(&store, Scheme::GitHub, &unnamed)),
        Ok(())
    );
}

struct Refusing(StoreError);

impl Store for Refusing {
    fn capabilities(&self) -> herald_ports::Capabilities {
        FakeStore::default().capabilities()
    }
    async fn entry(
        &self,
        _: &herald_core::Key,
    ) -> Result<Option<herald_ports::Versioned<herald_core::Entry>>, StoreError> {
        Err(self.0.clone())
    }
    async fn put_entry(
        &self,
        _: &herald_core::Entry,
        _: Option<herald_ports::Version>,
    ) -> Result<herald_ports::Written, StoreError> {
        Err(self.0.clone())
    }
    async fn cursor(
        &self,
        _: &herald_core::SourceName,
    ) -> Result<Option<herald_ports::Versioned<herald_core::Cursor>>, StoreError> {
        Err(self.0.clone())
    }
    async fn put_cursor(
        &self,
        _: &herald_core::SourceName,
        _: &herald_core::Cursor,
        _: Option<herald_ports::Version>,
    ) -> Result<herald_ports::Written, StoreError> {
        Err(self.0.clone())
    }
    async fn schedules(&self) -> Result<Vec<herald_core::Schedule>, StoreError> {
        Err(self.0.clone())
    }
    async fn put_schedules(
        &self,
        _: &[herald_core::Schedule],
        _: &[herald_core::Schedule],
    ) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
    async fn subscribers(
        &self,
        _: &herald_core::DestinationName,
        _: &str,
    ) -> Result<Vec<String>, StoreError> {
        Err(self.0.clone())
    }
    async fn subscribe(&self, _: &herald_core::Subscription) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
    async fn unsubscribe(&self, _: &herald_core::Subscription) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
    async fn pinned(&self, _: &herald_core::RouteId) -> Result<Option<String>, StoreError> {
        Err(self.0.clone())
    }
    async fn put_pinned(&self, _: &herald_core::RouteId, _: &str) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
    async fn admit(
        &self,
        _: Scheme,
        _: &str,
        _: Timestamp,
    ) -> Result<herald_ports::Admitted, StoreError> {
        Err(self.0.clone())
    }
    async fn release(&self, _: Scheme, _: &str) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
    async fn expire_deliveries(&self, _: Timestamp) -> Result<(), StoreError> {
        Err(self.0.clone())
    }
}

#[test]
fn a_store_keeping_no_deliveries_admits_every_one_and_the_ledger_skips_replays() {
    let store = Refusing(StoreError::Unsupported { what: "deliveries" });
    let verified = delivered("1");
    assert_eq!(
        pollster::block_on(admitted(&store, Scheme::GitHub, &verified, now())),
        Ok(())
    );
    assert_eq!(
        pollster::block_on(released(&store, Scheme::GitHub, &verified)),
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
    let rejected = pollster::block_on(admitted(&store, Scheme::GitHub, &verified, now()));
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
        pollster::block_on(released(&store, Scheme::GitHub, &verified)),
        Err(down)
    );
}
