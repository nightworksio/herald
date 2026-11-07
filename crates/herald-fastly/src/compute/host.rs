use std::collections::BTreeMap;
use std::future::{Future, ready};

use fastly::{ConfigStore, SecretStore};
use herald_compose::{EGRESS_GUARDED, GITHUB_REPOSITORY, Host, KvAt, Reaching, SqlAt, Unoffered};
use herald_crypto::{RingSigner, RingVerifier};
use herald_environment::SystemClock;
use herald_ports::{Secret, SecretError, Secrets};

use super::http::FastlyHttp;
use super::kv::FastlyKv;

/// The name of the Config Store and of the Secret Store herald reads.
pub(super) const STORE: &str = "herald";

/// The variables herald reads, kept in the Config Store since a Compute
/// service has no environment of its own.
const VARIABLES: [&str; 1] = [GITHUB_REPOSITORY];

/// Secrets from the Secret Store `herald`.
#[derive(Debug, Default)]
pub(super) struct StoreSecrets;

impl Secrets for StoreSecrets {
    fn secret(&self, name: &str) -> impl Future<Output = Result<Secret, SecretError>> + Send {
        let found = SecretStore::open(STORE)
            .ok()
            .and_then(|store| store.try_get(name).ok().flatten())
            .and_then(|secret| String::from_utf8(secret.plaintext().to_vec()).ok())
            .filter(|value| !value.is_empty())
            .map(|value| Secret::new(name, value))
            .ok_or_else(|| SecretError::Missing {
                name: name.to_owned(),
            });
        ready(found)
    }
}

/// A Compute service: its stores, and what reaches out through it.
pub(super) struct FastlyHost {
    pub(super) config: Option<ConfigStore>,
    pub(super) vars: BTreeMap<&'static str, String>,
    pub(super) secrets: StoreSecrets,
}

impl FastlyHost {
    pub(super) fn new() -> Self {
        let config = ConfigStore::try_open(STORE).ok();
        let vars = VARIABLES
            .into_iter()
            .filter_map(|name| {
                let value = config.as_ref()?.try_get(name).ok().flatten()?;
                Some((name, value))
            })
            .collect();
        Self {
            config,
            vars,
            secrets: StoreSecrets,
        }
    }

    /// The Config Store's item `name`.
    pub(super) fn item(&self, name: &str) -> Option<String> {
        self.config.as_ref()?.try_get(name).ok().flatten()
    }

    /// The secret `name`, where the Secret Store holds it.
    pub(super) async fn secrets_for_round(&self, name: &str) -> Option<Secret> {
        self.secrets.secret(name).await.ok()
    }
}

impl Host for FastlyHost {
    type Http = FastlyHttp;
    type Secrets = StoreSecrets;
    type Clock = SystemClock;
    type Signer = RingSigner;
    type Verifier = RingVerifier;
    type Sql = Unoffered;
    type Kv = FastlyKv;

    fn connect(&self, base: &str) -> Result<FastlyHttp, String> {
        Reaching::unresolved(base, self.item(EGRESS_GUARDED).as_deref()).map(FastlyHttp::new)
    }

    fn var(&self, name: &str) -> Option<&str> {
        self.vars
            .get(name)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
    }

    fn secrets(&self) -> &StoreSecrets {
        &self.secrets
    }

    fn clock(&self) -> &SystemClock {
        &SystemClock
    }

    fn signer(&self) -> &RingSigner {
        &RingSigner
    }

    fn verifier(&self) -> &RingVerifier {
        &RingVerifier
    }

    fn sql(&self, _: SqlAt<'_>) -> impl Future<Output = Result<Unoffered, String>> {
        ready(Err("Fastly Compute offers no database".to_owned()))
    }

    fn kv(&self, at: KvAt<'_>) -> impl Future<Output = Result<FastlyKv, String>> {
        ready(match at {
            KvAt::FastlyKv(name) => FastlyKv::open(name),
            KvAt::DenoKv(_) => Err("Fastly Compute offers no Deno KV".to_owned()),
        })
    }
}
