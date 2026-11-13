//! The native binary as the host herald is composed on, its stores reached
//! over the run's connections (ADR-0005, ADR-0017).

use std::collections::BTreeMap;
use std::future::{Future, ready};

use herald_compose::{Composed, Host, Hosting, KvAt, SqlAt, Unoffered};
use herald_config::{Config, Uses};
use herald_crypto::{RingSigner, RingVerifier};
use herald_environment::{EnvSecrets, SystemClock};
use herald_native::{NativePostgres, NativeSql, NativeSqlite, Timeouts};
use herald_ports::Outbound;

use super::{Connect, Connection};

/// The native binary, as the host its stores are reached through.
pub(crate) struct Native<'a> {
    vars: &'a BTreeMap<String, String>,
    secrets: EnvSecrets,
    connect: Connect<'a>,
}

impl<'a> Native<'a> {
    pub(crate) fn new(vars: &'a BTreeMap<String, String>, connect: Connect<'a>) -> Self {
        Self {
            vars,
            secrets: EnvSecrets::from_vars(vars.clone()),
            connect,
        }
    }
}

/// Why the native binary opens no database, or key-value store, at `at`.
const UNHOSTED: &str = "only a Cloudflare Worker, Deno or Fastly Compute reaches it";

impl Host for Native<'_> {
    type Http = Connection;
    type Secrets = EnvSecrets;
    type Clock = SystemClock;
    type Signer = RingSigner;
    type Verifier = RingVerifier;
    type Sql = NativeSql;
    type Kv = Unoffered;

    fn connect(&self, base: &str, outbound: &Outbound) -> Result<Connection, String> {
        self.connect.reach_within(base, outbound)
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

    async fn sql(&self, at: SqlAt<'_>) -> Result<NativeSql, String> {
        match at {
            SqlAt::Sqlite { file, busy } => NativeSqlite::open(file, busy).map(NativeSql::Sqlite),
            SqlAt::Postgres {
                url,
                connect,
                statement,
            } => NativePostgres::connect(url, Timeouts { connect, statement })
                .await
                .map(|sql| NativeSql::Postgres(Box::new(sql))),
            SqlAt::D1(_) | SqlAt::DurableObject { .. } => return Err(UNHOSTED.to_owned()),
        }
        .map_err(|error| error.to_string())
    }

    fn kv(&self, _: KvAt<'_>) -> impl Future<Output = Result<Unoffered, String>> {
        ready(Err(UNHOSTED.to_owned()))
    }
}

/// herald composed on the native binary as `hosting` describes it, reading
/// the secrets a run that `uses` the config reads, and taking leases under
/// the run name `run`.
pub(crate) async fn composed<'a>(
    config: &Config,
    vars: &'a BTreeMap<String, String>,
    connect: Connect<'a>,
    hosting: Hosting,
    uses: Uses,
    run: impl Into<String>,
) -> Result<Composed<'a, Native<'a>>, String> {
    Composed::new(
        Native::new(vars, connect),
        config.clone(),
        hosting,
        uses,
        run,
    )
    .await
}
