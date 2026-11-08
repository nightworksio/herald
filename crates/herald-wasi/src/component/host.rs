use std::collections::BTreeMap;
use std::future::{Future, ready};

use herald_compose::{EGRESS_GUARDED, Host, KvAt, Reaching, SqlAt, Unoffered, plain};
use herald_crypto::{RingSigner, RingVerifier};
use herald_environment::{EnvSecrets, SystemClock};

use herald_ports::Outbound;

use super::http::WasiHttp;

/// A `wasi:http` host: its environment, and what reaches out through it.
pub(super) struct WasiHost {
    vars: BTreeMap<String, String>,
    secrets: EnvSecrets,
}

impl WasiHost {
    /// The host whose environment is `vars`, as `(name, value)`.
    pub(super) fn new(vars: impl IntoIterator<Item = (String, String)>) -> Self {
        let vars: BTreeMap<String, String> = vars.into_iter().collect();
        Self {
            secrets: EnvSecrets::from_vars(vars.clone()),
            vars,
        }
    }
}

const NO_DATABASE: &str = "this host offers no database herald keeps records in";
/// The variable naming Spin's SQLite database where `default` is not the one.
#[cfg(feature = "spin")]
const SPIN_DATABASE: &str = "HERALD_SPIN_DATABASE";

#[cfg(feature = "spin")]
type Database = super::spin::SpinSql;
#[cfg(not(feature = "spin"))]
type Database = Unoffered;

impl Host for WasiHost {
    type Http = WasiHttp;
    type Secrets = EnvSecrets;
    type Clock = SystemClock;
    type Signer = RingSigner;
    type Verifier = RingVerifier;
    type Sql = Database;
    type Kv = Unoffered;

    fn connect(&self, base: &str, outbound: &Outbound) -> Result<WasiHttp, String> {
        plain(outbound, "a WASI component")?;
        Reaching::unresolved(base, self.var(EGRESS_GUARDED))
            .map(|reaching| WasiHttp::new(reaching, outbound.total.unsigned_abs()))
    }

    fn var(&self, name: &str) -> Option<&str> {
        self.vars
            .get(name)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
    }

    fn secrets(&self) -> &EnvSecrets {
        &self.secrets
    }

    fn clock(&self) -> &SystemClock {
        &SystemClock
    }

    fn nonce(&self) -> String {
        herald_environment::nonce()
    }

    fn signer(&self) -> &RingSigner {
        &RingSigner
    }

    fn verifier(&self) -> &RingVerifier {
        &RingVerifier
    }

    #[cfg(feature = "spin")]
    fn sql(&self, at: SqlAt<'_>) -> impl Future<Output = Result<Database, String>> {
        ready(match at {
            SqlAt::Sqlite { busy, .. } => Ok(Database::Sqlite {
                label: self.var(SPIN_DATABASE).unwrap_or("default").to_owned(),
                busy,
            }),
            SqlAt::Postgres {
                url,
                connect,
                statement,
            } => Ok(Database::Postgres(herald_ports::Secret::new(
                url.name(),
                herald_store_postgres::within(url.expose(), connect, statement),
            ))),
            SqlAt::D1(_) | SqlAt::DurableObject { .. } => Err(NO_DATABASE.to_owned()),
        })
    }

    #[cfg(not(feature = "spin"))]
    fn sql(&self, _: SqlAt<'_>) -> impl Future<Output = Result<Database, String>> {
        ready(Err(NO_DATABASE.to_owned()))
    }

    fn kv(&self, _: KvAt<'_>) -> impl Future<Output = Result<Unoffered, String>> {
        ready(Err(NO_DATABASE.to_owned()))
    }
}
