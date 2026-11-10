use std::future::{Future, ready};
use std::sync::atomic::{AtomicU64, Ordering};

use herald_kinds::{KvAt, SqlAt};
use herald_ports::{CryptoError, Http, HttpError, Request, Response, Secret, Signer, Verifier};
use herald_testing::{FakeClock, FakeKv, FakeSecrets, FakeSigner, FakeVerifier, ScriptedSql};

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    Conforming,
    MissesTheVariable,
    ReadsAnEmptyVariable,
    ReadsAnUnsetVariable,
    MissesTheSecret,
    HoldsAnUnsetSecret,
    RepeatsItsNonce,
    GivesNoNonce,
    IsBehind,
    RefusesToSign,
    VerifiesEverything,
    VerifiesNothing,
    ReachesNothing,
}

#[derive(Clone)]
struct Unsent;

impl Http for Unsent {
    fn send(&self, request: Request) -> impl Future<Output = Result<Response, HttpError>> + Send {
        ready(Err(HttpError::Refused {
            url: request.url.redacted(),
        }))
    }
}

#[derive(Clone)]
struct Signing(Fault);

impl Signer for Signing {
    async fn sign(
        &self,
        algorithm: Algorithm,
        key: &Secret,
        message: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        if self.0 == Fault::RefusesToSign {
            return Err(CryptoError::BadKey {
                name: key.name().to_owned(),
            });
        }
        FakeSigner.sign(algorithm, key, message).await
    }

    async fn public(&self, algorithm: Algorithm, key: &Secret) -> Result<Vec<u8>, CryptoError> {
        FakeSigner.public(algorithm, key).await
    }
}

impl Verifier for Signing {
    async fn verify(
        &self,
        algorithm: Algorithm,
        key: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> bool {
        match self.0 {
            Fault::VerifiesEverything => true,
            Fault::VerifiesNothing => false,
            _ => {
                FakeVerifier
                    .verify(algorithm, key, message, signature)
                    .await
            }
        }
    }
}

struct Hosted {
    fault: Fault,
    secrets: FakeSecrets,
    clock: FakeClock,
    signing: Signing,
    nonces: AtomicU64,
}

fn hosted(fault: Fault) -> Hosted {
    let mut secrets = FakeSecrets::default();
    if fault != Fault::MissesTheSecret {
        secrets = secrets.with(CONFORMANCE_SECRET, CONFORMS);
    }
    if fault == Fault::HoldsAnUnsetSecret {
        secrets = secrets.with(UNSET, CONFORMS);
    }
    let now = if fault == Fault::IsBehind {
        WRITTEN - jiff::SignedDuration::from_secs(1)
    } else {
        WRITTEN
    };
    Hosted {
        fault,
        secrets,
        clock: FakeClock::at(now),
        signing: Signing(fault),
        nonces: AtomicU64::new(1),
    }
}

impl Host for Hosted {
    type Http = Unsent;
    type Secrets = FakeSecrets;
    type Clock = FakeClock;
    type Signer = Signing;
    type Verifier = Signing;
    type Sql = ScriptedSql;
    type Kv = FakeKv;

    fn connect(&self, base: &str, _: &Outbound) -> Result<Unsent, String> {
        if self.fault == Fault::ReachesNothing {
            return Err(format!("{base} is out of reach"));
        }
        Ok(Unsent)
    }

    fn var(&self, name: &str) -> Option<&str> {
        match (name, self.fault) {
            (CONFORMANCE_VAR, fault) if fault != Fault::MissesTheVariable => Some(CONFORMS),
            (CONFORMANCE_EMPTY, Fault::ReadsAnEmptyVariable) => Some(""),
            (UNSET, Fault::ReadsAnUnsetVariable) => Some(CONFORMS),
            _ => None,
        }
    }

    fn secrets(&self) -> &FakeSecrets {
        &self.secrets
    }

    fn clock(&self) -> &FakeClock {
        &self.clock
    }

    fn signer(&self) -> &Signing {
        &self.signing
    }

    fn verifier(&self) -> &Signing {
        &self.signing
    }

    fn sql(&self, _: SqlAt<'_>) -> impl Future<Output = Result<ScriptedSql, String>> {
        ready(Ok(ScriptedSql::default()))
    }

    fn kv(&self, _: KvAt<'_>) -> impl Future<Output = Result<FakeKv, String>> {
        ready(Ok(FakeKv::default()))
    }

    fn nonce(&self) -> String {
        match self.fault {
            Fault::GivesNoNonce => String::new(),
            Fault::RepeatsItsNonce => "1".to_owned(),
            _ => self.nonces.fetch_add(1, Ordering::Relaxed).to_string(),
        }
    }
}

fn run(fault: Fault) -> Result<(), Failure> {
    pollster::block_on(host_conformance(&hosted(fault)))
}

#[test]
fn a_host_keeping_every_promise_passes() {
    assert_eq!(run(Fault::Conforming), Ok(()));
}

#[test]
fn a_host_breaking_a_promise_is_refused() {
    let cases = [
        (
            Fault::MissesTheVariable,
            "a variable the platform sets was not read as set",
        ),
        (
            Fault::ReadsAnEmptyVariable,
            "a variable set empty was read as set",
        ),
        (
            Fault::ReadsAnUnsetVariable,
            "a variable the platform does not set was read",
        ),
        (
            Fault::MissesTheSecret,
            "a secret the platform holds was not read as held",
        ),
        (
            Fault::HoldsAnUnsetSecret,
            "a secret the platform does not hold was not refused as missing",
        ),
        (Fault::RepeatsItsNonce, "two nonces were alike"),
        (Fault::GivesNoNonce, "two nonces were alike"),
        (
            Fault::IsBehind,
            "the clock is behind the time the host suite was written",
        ),
        (
            Fault::RefusesToSign,
            "a signature the host made was not verified for its message alone",
        ),
        (
            Fault::VerifiesEverything,
            "a signature the host made was not verified for its message alone",
        ),
        (
            Fault::VerifiesNothing,
            "a signature the host made was not verified for its message alone",
        ),
        (Fault::ReachesNothing, "an https base could not be reached"),
    ];
    for (fault, failure) in cases {
        assert_eq!(run(fault), Err(Failure(failure.to_owned())), "{failure}");
    }
}

#[test]
fn a_host_holding_its_secret_at_another_value_is_refused() {
    let misnamed = Hosted {
        secrets: FakeSecrets::default().with(CONFORMANCE_SECRET, "other"),
        ..hosted(Fault::Conforming)
    };
    assert_eq!(
        pollster::block_on(host_conformance(&misnamed)),
        Err(Failure(
            "a secret the platform holds was not read as held".to_owned()
        ))
    );
}
