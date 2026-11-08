//! The store a run keeps its records in, as the config names it, reached
//! over the run's connections (ADR-0005).

use std::collections::BTreeMap;
use std::future::{Future, ready};

use herald_compose::{Host, KvAt, Served, SqlAt, Unoffered};
use herald_config::{Config, Store};
use herald_crypto::{RingSigner, RingVerifier};
use herald_environment::{EnvSecrets, SystemClock};
use herald_flows::{Connected, prepare};
use herald_native::{NativePostgres, NativeSql, NativeSqlite, Timeouts};
use herald_ports::{Outbound, Secrets as _};
use herald_records::{Memory, Seal};
use herald_store_actions_cache::{ActionsCache, ActionsCacheStore, StoreOptions as CacheOptions};

use super::{Connect, Connection, RESULTS, RUNTIME_TOKEN, variable};

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
pub(crate) enum Records<'a> {
    /// In a store every host reaches alike.
    Hosted(herald_compose::Records<Native<'a>>),
    /// In the Actions cache.
    Cache(ActionsCacheStore<'a, Connection>),
    /// In memory, for a dry run.
    Memory(herald_records::Kept<Memory>),
}

impl Records<'_> {
    /// `work` done over the store, the routes' posts made one where they
    /// keep the records.
    pub(crate) async fn serve<W: Served>(
        &self,
        connected: &Connected<Connection>,
        work: W,
    ) -> W::Done {
        match self {
            Self::Hosted(records) => records.serve(connected, &RingSigner, work).await,
            Self::Cache(store) => work.over(store).await,
            Self::Memory(store) => work.over(store).await,
        }
    }
}

/// The store kinds `herald server` keeps records in: every kind but the
/// Actions cache, which only a workflow run reaches.
pub(crate) const SERVED: [&str; 6] = [
    Store::SQLITE,
    Store::POSTGRES,
    Store::DYNAMODB,
    Store::GIT_BRANCH,
    Store::MARKERS,
    Store::REPO_VARIABLES,
];

/// The store kinds a Lambda function keeps records in: every kind that
/// outlives the function's file system and needs no workflow run.
pub(crate) const LAMBDA: [&str; 5] = [
    Store::POSTGRES,
    Store::DYNAMODB,
    Store::GIT_BRANCH,
    Store::MARKERS,
    Store::REPO_VARIABLES,
];

/// The store kinds this herald keeps records in.
pub(crate) const STORES: [&str; 7] = [
    Store::SQLITE,
    Store::POSTGRES,
    Store::DYNAMODB,
    Store::GIT_BRANCH,
    Store::MARKERS,
    Store::ACTIONS_CACHE,
    Store::REPO_VARIABLES,
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
    let native = Native::new(vars, connect);
    let records = match &config.store {
        Store::ActionsCache(options) if stores.contains(&Store::ACTIONS_CACHE) => {
            Records::Cache(cache(options, vars, &native.secrets, connect).await?)
        }
        store => Records::Hosted(
            herald_compose::reached(&native, store, connect.outbound(), stores).await?,
        ),
    };
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

pub(crate) async fn cache<'a>(
    options: &CacheOptions,
    vars: &BTreeMap<String, String>,
    secrets: &EnvSecrets,
    connect: Connect<'a>,
) -> Result<ActionsCacheStore<'a, Connection>, String> {
    let results = variable(vars, RESULTS)?;
    let key = secrets.secret(&options.key.secret).await.map_err(|_| {
        format!(
            "{} is not set; the store seals its records with it",
            options.key.secret
        )
    })?;
    let token = secrets.secret(RUNTIME_TOKEN).await.map_err(|_| {
        format!("{RUNTIME_TOKEN} is not set; the actions-cache store runs through herald's Action")
    })?;
    let cache =
        ActionsCache::new(results, token, options.prefix.as_deref()).keeping(&options.keeps);
    let http = connect.reach(cache.results())?;
    Ok(cache
        .store(http, Box::new(move |base: &str| connect.reach(base)))
        .sealed(Seal::new(key)))
}
