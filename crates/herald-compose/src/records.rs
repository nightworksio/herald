use std::future::Future;
use std::path::Path;

use herald_config::Store;
use herald_flows::{Connected, unserved};
use herald_ports::{Keyring, Outbound, Secret, Secrets as _, Store as Keeps};
use herald_records::{Kept, KvRecords, Seal, SqlRecords};
use herald_store_dynamodb::{Dynamodb, DynamodbStore};
use herald_store_git_branch::{GitBranch, GitBranchStore, StoreOptions as BranchOptions};
use herald_store_repo_variables::{
    RepoVariables, RepoVariablesStore, StoreOptions as VariablesOptions,
};
use herald_store_sqlite::{SqliteStore, StoreOptions as SqliteOptions};

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
    outbound: &Outbound,
    stores: &[&str],
) -> Result<Records<H>, String> {
    if !stores.contains(&store.kind()) {
        return Err(unserved(stores));
    }
    let seal = match store.key() {
        Some(key) => Some(Seal::new(
            secret(host, &key.secret, "the store seals its records with it").await?,
        )),
        None => None,
    };
    opening(host, store, outbound, stores, seal).await
}

async fn opening<H: Host>(
    host: &H,
    store: &Store,
    outbound: &Outbound,
    stores: &[&str],
    seal: Option<Seal>,
) -> Result<Records<H>, String> {
    let kind = store.kind();
    let opened = |error: String| format!("the {kind} store could not be opened: {error}");
    let migrated = |error: herald_ports::StoreError| opened(error.to_string());
    if let Some(options) = store.of::<SqliteOptions>() {
        return Ok(Records::Sql(sqlite(host, options).await?.sealed(seal)));
    }
    if let Some(options) = store.of::<herald_store_postgres::StoreOptions>() {
        let name = options.url.secret.as_str();
        let url = secret(
            host,
            name,
            "the postgres store reads its connection URL from it",
        )
        .await?;
        let at = SqlAt::Postgres {
            url: &url,
            connect: options.connect_timeout(),
            statement: options.statement_timeout(),
        };
        let sql = host.sql(at).await.map_err(opened)?;
        return Ok(Records::Sql(
            herald_store_postgres::store(sql)
                .await
                .map_err(migrated)?
                .sealed(seal),
        ));
    }
    if let Some(options) = store.of::<herald_store_d1::StoreOptions>() {
        let sql = host
            .sql(SqlAt::D1(options.binding()))
            .await
            .map_err(opened)?;
        return Ok(Records::Sql(
            herald_store_d1::store(sql)
                .await
                .map_err(migrated)?
                .sealed(seal),
        ));
    }
    if let Some(options) = store.of::<herald_store_durable_object::StoreOptions>() {
        let at = SqlAt::DurableObject {
            binding: options.binding(),
            object: options.object(),
        };
        let sql = host.sql(at).await.map_err(opened)?;
        return Ok(Records::Sql(
            herald_store_durable_object::store(sql)
                .await
                .map_err(migrated)?
                .sealed(seal),
        ));
    }
    if let Some(options) = store.of::<herald_store_deno_kv::StoreOptions>() {
        let kv = host
            .kv(KvAt::DenoKv(options.path.as_deref()))
            .await
            .map_err(opened)?;
        return Ok(Records::Kv(
            herald_store_deno_kv::store(kv, options).sealed(seal),
        ));
    }
    if let Some(options) = store.of::<herald_store_fastly_kv::StoreOptions>() {
        let kv = host
            .kv(KvAt::FastlyKv(options.store()))
            .await
            .map_err(opened)?;
        return Ok(Records::Kv(
            herald_store_fastly_kv::store(kv, options).sealed(seal),
        ));
    }
    if let Some(options) = store.of::<BranchOptions>() {
        return Ok(Records::Branch(
            branch(host, options, outbound).await?.sealed(seal),
        ));
    }
    if let Some(options) = store.of::<VariablesOptions>() {
        return Ok(Records::Variables(
            variables(host, options, outbound).await?.sealed(seal),
        ));
    }
    if let Some(options) = store.of::<herald_store_dynamodb::StoreOptions>() {
        let table = Dynamodb::reached(options, host.secrets()).await?;
        let http = host.connect(table.api(), outbound)?;
        return Ok(Records::Dynamodb(
            table
                .store(http, host.clock().clone(), host.nonce())
                .sealed(seal),
        ));
    }
    if store.reads_posts() {
        return Ok(Records::Posts);
    }
    Err(unserved(stores))
}

async fn sqlite<H: Host>(host: &H, options: &SqliteOptions) -> Result<SqliteStore<H::Sql>, String> {
    let file = options.file(Path::new(""));
    let filed = |error: String| {
        format!(
            "the sqlite store {} could not be opened: {error}",
            file.display()
        )
    };
    let at = SqlAt::Sqlite {
        file: &file,
        busy: options.busy(),
    };
    let sql = host.sql(at).await.map_err(filed)?;
    herald_store_sqlite::store(sql)
        .await
        .map_err(|error| filed(error.to_string()))
}

async fn branch<H: Host>(
    host: &H,
    options: &BranchOptions,
    outbound: &Outbound,
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
    let http = host.connect(branch.api(), outbound)?;
    Ok(branch.store(http))
}

async fn variables<H: Host>(
    host: &H,
    options: &VariablesOptions,
    outbound: &Outbound,
) -> Result<RepoVariablesStore<H::Http>, String> {
    let repository = repository(host, options.repository.as_deref())?;
    let token = secret(
        host,
        options.token.secret.as_str(),
        "the repo-variables store needs a token with the repository's Variables permission",
    )
    .await?;
    let variables = RepoVariables::new(repository, token, options.api.as_deref());
    let http = host.connect(variables.api(), outbound)?;
    Ok(variables.store(http, host.nonce()))
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
