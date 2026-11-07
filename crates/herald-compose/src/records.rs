use std::future::Future;
use std::path::Path;

use herald_config::Store;
use herald_flows::{Connected, unserved};
use herald_ports::{Keyring, Secret, Secrets as _, Store as Keeps};
use herald_records::{Kept, KvRecords, SqlRecords};
use herald_store_dynamodb::{Dynamodb, DynamodbStore};
use herald_store_git_branch::{GitBranch, GitBranchStore, StoreOptions as BranchOptions};
use herald_store_repo_variables::{
    RepoVariables, RepoVariablesStore, StoreOptions as VariablesOptions,
};

use crate::host::{Host, KvAt, SqlAt};

/// The token a store reaching GitHub falls back on, as GitHub Actions sets it.
pub const GITHUB_TOKEN: &str = "GITHUB_TOKEN";
/// The repository a store on GitHub falls back on, as GitHub Actions sets it.
pub const GITHUB_REPOSITORY: &str = "GITHUB_REPOSITORY";

/// Where a host's run keeps its records, as the config names it.
pub enum Records<H: Host> {
    /// In a database: SQLite, Postgres, D1 or a Durable Object's storage.
    Sql(Kept<SqlRecords<H::Sql>>),
    /// In a key-value store: Deno KV or a Fastly KV Store.
    Kv(Kept<KvRecords<H::Kv>>),
    /// On a git branch.
    Branch(GitBranchStore<H::Http>),
    /// In the repository's variables.
    Variables(RepoVariablesStore<H::Http>),
    /// In a DynamoDB table.
    Dynamodb(DynamodbStore<H::Http, H::Clock>),
    /// In the routes' own posts.
    Posts,
}

/// Work done over whichever store the config names.
pub trait Served {
    /// What the work did.
    type Done;

    /// The work done over `store`.
    fn over<S: Keeps + Keyring>(self, store: &S) -> impl Future<Output = Self::Done>;
}

impl<H: Host> Records<H> {
    /// `work` done over the store, the routes' posts, signed by `signer`,
    /// made one where they keep the records.
    pub async fn serve<W: Served>(
        &self,
        connected: &Connected<H::Http>,
        signer: &H::Signer,
        work: W,
    ) -> W::Done {
        match self {
            Self::Sql(store) => work.over(store).await,
            Self::Kv(store) => work.over(store).await,
            Self::Branch(store) => work.over(store).await,
            Self::Variables(store) => work.over(store).await,
            Self::Dynamodb(store) => work.over(store).await,
            Self::Posts => work.over(&connected.marked(signer.clone())).await,
        }
    }
}

/// The store `store` names, reached through `host`, where it is one of the
/// kinds in `stores`.
///
/// # Errors
///
/// Where the store is not one of `stores`, or cannot be reached, saying why.
pub async fn reached<H: Host>(
    host: &H,
    store: &Store,
    stores: &[&str],
) -> Result<Records<H>, String> {
    if !stores.contains(&store.kind()) {
        return Err(unserved(stores));
    }
    let kind = store.kind();
    let opened = |error: String| format!("the {kind} store could not be opened: {error}");
    let migrated = |error: herald_ports::StoreError| opened(error.to_string());
    Ok(match store {
        Store::Sqlite(options) => {
            let file = options.file(Path::new(""));
            let filed = |error: String| {
                format!(
                    "the sqlite store {} could not be opened: {error}",
                    file.display()
                )
            };
            let sql = host.sql(SqlAt::Sqlite(&file)).await.map_err(filed)?;
            Records::Sql(
                herald_store_sqlite::store(sql)
                    .await
                    .map_err(|error| filed(error.to_string()))?,
            )
        }
        Store::Postgres(options) => {
            let name = options.url.secret.as_str();
            let url = secret(
                host,
                name,
                "the postgres store reads its connection URL from it",
            )
            .await?;
            let sql = host.sql(SqlAt::Postgres(&url)).await.map_err(opened)?;
            Records::Sql(herald_store_postgres::store(sql).await.map_err(migrated)?)
        }
        Store::D1(options) => {
            let sql = host
                .sql(SqlAt::D1(options.binding()))
                .await
                .map_err(opened)?;
            Records::Sql(herald_store_d1::store(sql).await.map_err(migrated)?)
        }
        Store::DurableObject(options) => {
            let at = SqlAt::DurableObject {
                binding: options.binding(),
                object: options.object(),
            };
            let sql = host.sql(at).await.map_err(opened)?;
            Records::Sql(
                herald_store_durable_object::store(sql)
                    .await
                    .map_err(migrated)?,
            )
        }
        Store::DenoKv(options) => {
            let kv = host
                .kv(KvAt::DenoKv(options.path.as_deref()))
                .await
                .map_err(opened)?;
            Records::Kv(herald_store_deno_kv::store(kv, options))
        }
        Store::FastlyKv(options) => {
            let kv = host
                .kv(KvAt::FastlyKv(options.store()))
                .await
                .map_err(opened)?;
            Records::Kv(herald_store_fastly_kv::store(kv, options))
        }
        Store::GitBranch(options) => Records::Branch(branch(host, options).await?),
        Store::RepoVariables(options) => Records::Variables(variables(host, options).await?),
        Store::Dynamodb(options) => {
            let table = Dynamodb::reached(options, host.secrets()).await?;
            let http = host.connect(table.api())?;
            Records::Dynamodb(table.store(http, host.clock().clone()))
        }
        Store::Markers => Records::Posts,
        Store::ActionsCache(_) => return Err(unserved(stores)),
    })
}

async fn branch<H: Host>(
    host: &H,
    options: &BranchOptions,
) -> Result<GitBranchStore<H::Http>, String> {
    let repository = repository(host, options.repository.as_deref())?;
    let name = options
        .token
        .as_ref()
        .map_or(GITHUB_TOKEN, |token| token.secret.as_str());
    let token = secret(
        host,
        name,
        "the git-branch store needs a token that may push",
    )
    .await?;
    let branch = GitBranch::new(
        repository,
        token,
        options.branch.as_deref(),
        options.api.as_deref(),
    );
    let http = host.connect(branch.api())?;
    Ok(branch.store(http))
}

async fn variables<H: Host>(
    host: &H,
    options: &VariablesOptions,
) -> Result<RepoVariablesStore<H::Http>, String> {
    let repository = repository(host, options.repository.as_deref())?;
    let token = secret(
        host,
        options.token.secret.as_str(),
        "the repo-variables store needs a token with the repository's Variables permission",
    )
    .await?;
    let variables = RepoVariables::new(repository, token, options.api.as_deref());
    let http = host.connect(variables.api())?;
    Ok(variables.store(http))
}

/// The repository a store names, or else the one the host runs for.
fn repository<'a, H: Host>(host: &'a H, named: Option<&'a str>) -> Result<&'a str, String> {
    named
        .or_else(|| host.var(GITHUB_REPOSITORY))
        .ok_or_else(|| {
            format!("{GITHUB_REPOSITORY} is not set; name the store's repository, or run herald in the repository's CI")
        })
}

async fn secret<H: Host>(host: &H, name: &str, why: &str) -> Result<Secret, String> {
    host.secrets()
        .secret(name)
        .await
        .map_err(|_| format!("{name} is not set; {why}"))
}
