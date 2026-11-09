//! The store a run keeps its records in, as the config names it, reached
//! over the run's connections (ADR-0005).

use std::collections::BTreeMap;
use std::future::{Future, ready};
use std::sync::Arc;

use herald_compose::{Host, KvAt, Opener, Served, SqlAt, Unoffered};
use herald_config::Config;
use herald_crypto::{RingSigner, RingVerifier};
use herald_environment::{EnvSecrets, SystemClock};
use herald_flows::{Connected, prepare};
use herald_kinds::{Opened, StoreKind};
use herald_native::{NativePostgres, NativeSql, NativeSqlite, Timeouts};
use herald_ports::Outbound;
use herald_records::{Kept, Memory};
use herald_store_actions_cache::ActionsCacheKind;
use herald_store_dynamodb::DynamodbKind;
use herald_store_git_branch::GitBranchKind;
use herald_store_markers::MarkersKind;
use herald_store_postgres::PostgresKind;
use herald_store_repo_variables::RepoVariablesKind;
use herald_store_sqlite::SqliteKind;

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

/// Where the run's records are kept.
pub(crate) struct Records<'a>(herald_compose::Records<'a>);

impl Records<'_> {
    /// Records kept in memory, for a dry run.
    pub(crate) fn memory() -> Self {
        Self(herald_compose::Records::new(Opened::Kept(Box::new(
            Kept::new(Memory::default()),
        ))))
    }

    /// `work` done over the store, the routes' posts made one where they
    /// keep the records.
    pub(crate) async fn serve<W: Served>(
        &self,
        connected: &Connected<Connection>,
        work: W,
    ) -> W::Done {
        self.0.serve(connected, &RingSigner, work).await
    }
}

/// The store kinds `herald server` keeps records in: every kind but the
/// Actions cache, which only a workflow run reaches.
pub(crate) const SERVED: [&str; 6] = [
    SqliteKind::NAME,
    PostgresKind::NAME,
    DynamodbKind::NAME,
    GitBranchKind::NAME,
    MarkersKind::NAME,
    RepoVariablesKind::NAME,
];

/// The store kinds a Lambda function keeps records in: every kind that
/// outlives the function's file system and needs no workflow run.
pub(crate) const LAMBDA: [&str; 5] = [
    PostgresKind::NAME,
    DynamodbKind::NAME,
    GitBranchKind::NAME,
    MarkersKind::NAME,
    RepoVariablesKind::NAME,
];

/// The store kinds this herald keeps records in.
pub(crate) const STORES: [&str; 7] = [
    SqliteKind::NAME,
    PostgresKind::NAME,
    DynamodbKind::NAME,
    GitBranchKind::NAME,
    MarkersKind::NAME,
    ActionsCacheKind::NAME,
    RepoVariablesKind::NAME,
];

/// Whether a run answers the commands members give.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Commands {
    /// It answers them, so it reads the secrets each destination answers
    /// with.
    Answered,
    /// It answers none, and reads none of those secrets.
    Unanswered,
}

/// The run's records, in a store of one of the kinds in `stores`, and
/// every route and source connected, with every destination that takes
/// commands where the run answers them.
pub(crate) async fn reached<'a>(
    config: &Config,
    vars: &'a BTreeMap<String, String>,
    connect: Connect<'a>,
    commands: Commands,
    stores: &[&str],
) -> Result<(Records<'a>, Connected<Connection>), String> {
    let native = Arc::new(Native::new(vars, connect));
    let opener = Opener::new(Arc::clone(&native), connect.outbound().clone());
    let records = Records(herald_compose::reached(Arc::new(opener), &config.store, stores).await?);
    let prepared = prepare(config, &native.secrets, &RingSigner)
        .await
        .map_err(|error| error.to_string())?;
    let prepared = match commands {
        Commands::Answered => prepared
            .answering(&native.secrets)
            .await
            .map_err(|error| error.to_string())?,
        Commands::Unanswered => prepared,
    };
    Ok((records, prepared.connect(|base| connect.reach(base))?))
}
