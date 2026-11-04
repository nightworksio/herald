//! The store a run keeps its records in, as the config names it, reached
//! over the run's connections (ADR-0005).

use std::collections::BTreeMap;
use std::future::Future;
use std::path::Path;

use herald_config::{Config, Store};
use herald_flows::{Connected, prepare, unserved};
use herald_native::{EnvSecrets, NativePostgres, NativeSqlite, RingSigner};
use herald_ports::{Keyring, Secrets as _, Store as Keeps};
use herald_records::{Memory, MemoryKeyring};
use herald_store_actions_cache::{ActionsCache, ActionsCacheStore, StoreOptions as CacheOptions};
use herald_store_git_branch::{GitBranch, GitBranchStore, StoreOptions as BranchOptions};
use herald_store_postgres::{PostgresStore, StoreOptions as PostgresOptions};
use herald_store_repo_variables::{
    RepoVariables, RepoVariablesStore, StoreOptions as VariablesOptions,
};
use herald_store_sqlite::{SqliteStore, StoreOptions as SqliteOptions};

use super::{Connect, Connection, GITHUB_TOKEN, RESULTS, RUNTIME_TOKEN, Served, variable};

/// Where the run's records are kept.
pub(crate) enum Records<'a> {
    /// On a git branch.
    Branch(GitBranchStore<Connection>),
    /// In the Actions cache.
    Cache(ActionsCacheStore<'a, Connection>),
    /// In the repository's variables.
    Variables(RepoVariablesStore<Connection>),
    /// In a SQLite file.
    Sqlite(SqliteStore<NativeSqlite>),
    /// In a Postgres database.
    Postgres(PostgresStore<NativePostgres>),
    /// In the routes' own posts.
    Posts,
    /// In memory, for a dry run.
    Memory(herald_records::Kept<Memory>),
}

impl Records<'_> {
    /// `work` done over the store, the routes' posts made one where they
    /// keep the records.
    pub(crate) async fn serve<W: Served>(
        &self,
        connected: &Connected<Connection>,
        work: &W,
    ) -> W::Done {
        match self {
            Self::Branch(store) => work.over(store).await,
            Self::Cache(store) => work.over(store).await,
            Self::Variables(store) => work.over(store).await,
            Self::Sqlite(store) => work.over(store).await,
            Self::Postgres(store) => work.over(store).await,
            Self::Posts => work.over(&connected.marked(RingSigner)).await,
            Self::Memory(store) => work.over(store).await,
        }
    }
}

/// A server's work, over whichever store the config names and the keys it
/// checks activities with.
pub(crate) trait Keyed {
    /// What the work did.
    type Done;

    /// The work done over `store`, with keys kept in `keyring`.
    fn over<S: Keeps, K: Keyring>(self, store: &S, keyring: &K)
    -> impl Future<Output = Self::Done>;
}

/// `work` done over `store`, with keys kept in `store` itself where `kept`
/// says so, and in `memory` otherwise.
async fn keyed<S: Keeps + Keyring, W: Keyed>(
    store: &S,
    kept: bool,
    memory: &MemoryKeyring,
    work: W,
) -> W::Done {
    if kept {
        work.over(store, store).await
    } else {
        work.over(store, memory).await
    }
}

impl Records<'_> {
    /// `work` done over the store, with keys kept in the store where `kept`
    /// says so, and otherwise in memory, at most `most` of them.
    pub(crate) async fn serve_keyed<W: Keyed>(
        &self,
        connected: &Connected<Connection>,
        work: W,
        kept: bool,
        most: usize,
    ) -> W::Done {
        let memory = MemoryKeyring::new(most);
        match self {
            Self::Branch(store) => keyed(store, kept, &memory, work).await,
            Self::Cache(store) => keyed(store, kept, &memory, work).await,
            Self::Variables(store) => keyed(store, kept, &memory, work).await,
            Self::Sqlite(store) => keyed(store, kept, &memory, work).await,
            Self::Postgres(store) => keyed(store, kept, &memory, work).await,
            Self::Posts => work.over(&connected.marked(RingSigner), &memory).await,
            Self::Memory(store) => keyed(store, kept, &memory, work).await,
        }
    }
}

/// The store kinds `herald server` keeps records in: every kind but the
/// Actions cache, which only a workflow run reaches.
pub(crate) const SERVED: [&str; 5] = [
    Store::SQLITE,
    Store::POSTGRES,
    Store::GIT_BRANCH,
    Store::MARKERS,
    Store::REPO_VARIABLES,
];

/// The store kinds this herald keeps records in.
pub(crate) const STORES: [&str; 6] = [
    Store::SQLITE,
    Store::POSTGRES,
    Store::GIT_BRANCH,
    Store::MARKERS,
    Store::ACTIONS_CACHE,
    Store::REPO_VARIABLES,
];

/// The run's records, and every route and source connected.
pub(crate) async fn reached<'a>(
    config: &Config,
    vars: &BTreeMap<String, String>,
    connect: Connect<'a>,
) -> Result<(Records<'a>, Connected<Connection>), String> {
    let secrets = EnvSecrets::from_vars(vars.clone());
    let records = match &config.store {
        Store::GitBranch(options) => {
            Records::Branch(branch(options, vars, &secrets, connect).await?)
        }
        Store::ActionsCache(options) => {
            Records::Cache(cache(options, vars, &secrets, connect).await?)
        }
        Store::RepoVariables(options) => {
            Records::Variables(variables(options, vars, &secrets, connect).await?)
        }
        Store::Sqlite(options) => Records::Sqlite(sqlite(options).await?),
        Store::Postgres(options) => Records::Postgres(postgres(options, &secrets).await?),
        Store::Markers => Records::Posts,
        _ => return Err(unserved(&STORES)),
    };
    let prepared = prepare(config, &secrets, &RingSigner)
        .await
        .map_err(|error| error.to_string())?;
    Ok((records, prepared.connect(connect)?))
}

/// The repository a store names, or else the run's own.
pub(crate) fn repository<'a>(
    named: Option<&'a str>,
    vars: &'a BTreeMap<String, String>,
) -> Result<&'a str, String> {
    named.map_or_else(|| variable(vars, "GITHUB_REPOSITORY"), Ok)
}

pub(crate) async fn branch(
    options: &BranchOptions,
    vars: &BTreeMap<String, String>,
    secrets: &EnvSecrets,
    connect: Connect<'_>,
) -> Result<GitBranchStore<Connection>, String> {
    let repository = repository(options.repository.as_deref(), vars)?;
    let token_name = options
        .token
        .as_ref()
        .map_or(GITHUB_TOKEN, |token| token.secret.as_str());
    let token = secrets.secret(token_name).await.map_err(|_| {
        format!("{token_name} is not set; the git-branch store needs a token that may push")
    })?;
    let branch = GitBranch::new(
        repository,
        token,
        options.branch.as_deref(),
        options.api.as_deref(),
    );
    let http = connect(branch.api())?;
    Ok(branch.store(http))
}

/// The store in the file `options` name, as [`crate::read`] placed it.
pub(crate) async fn sqlite(options: &SqliteOptions) -> Result<SqliteStore<NativeSqlite>, String> {
    let file = options.file(Path::new(""));
    let opened = |error: String| {
        format!(
            "the sqlite store {} could not be opened: {error}",
            file.display()
        )
    };
    let sql = NativeSqlite::open(&file).map_err(|error| opened(error.to_string()))?;
    herald_store_sqlite::store(sql)
        .await
        .map_err(|error| opened(error.to_string()))
}

/// The store in the database whose connection URL `options` name.
pub(crate) async fn postgres(
    options: &PostgresOptions,
    secrets: &EnvSecrets,
) -> Result<PostgresStore<NativePostgres>, String> {
    let name = options.url.secret.as_str();
    let url = secrets.secret(name).await.map_err(|_| {
        format!("{name} is not set; the postgres store reads its connection URL from it")
    })?;
    let opened = |error: String| format!("the postgres store could not be opened: {error}");
    let sql = NativePostgres::connect(&url)
        .await
        .map_err(|error| opened(error.to_string()))?;
    herald_store_postgres::store(sql)
        .await
        .map_err(|error| opened(error.to_string()))
}

pub(crate) async fn cache<'a>(
    options: &CacheOptions,
    vars: &BTreeMap<String, String>,
    secrets: &EnvSecrets,
    connect: Connect<'a>,
) -> Result<ActionsCacheStore<'a, Connection>, String> {
    let results = variable(vars, RESULTS)?;
    let token = secrets.secret(RUNTIME_TOKEN).await.map_err(|_| {
        format!("{RUNTIME_TOKEN} is not set; the actions-cache store runs through herald's Action")
    })?;
    let cache = ActionsCache::new(results, token, options.prefix.as_deref());
    let http = connect(cache.results())?;
    Ok(cache.store(http, Box::new(connect)))
}

pub(crate) async fn variables(
    options: &VariablesOptions,
    vars: &BTreeMap<String, String>,
    secrets: &EnvSecrets,
    connect: Connect<'_>,
) -> Result<RepoVariablesStore<Connection>, String> {
    let repository = repository(options.repository.as_deref(), vars)?;
    let name = options.token.secret.as_str();
    let token = secrets.secret(name).await.map_err(|_| {
        format!("{name} is not set; the repo-variables store needs a token with the repository's Variables permission")
    })?;
    let variables = RepoVariables::new(repository, token, options.api.as_deref());
    let http = connect(variables.api())?;
    Ok(variables.store(http))
}
