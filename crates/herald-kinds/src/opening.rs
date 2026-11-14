use std::fmt;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use herald_ports::{Clock, DynHttp, DynKv, DynSecrets, DynSql, DynStore, Owned, Secret};
use jiff::SignedDuration;

/// The token a store reaching GitHub falls back on, as GitHub Actions sets it.
pub const GITHUB_TOKEN: &str = "GITHUB_TOKEN";

/// The variable naming the repository a run is for, as GitHub Actions sets
/// it.
pub const GITHUB_REPOSITORY: &str = "GITHUB_REPOSITORY";

/// A future a store opens through: it may hold what only the host's own
/// thread may, as a WebAssembly host's databases do.
pub type Local<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// What a host gives a store as it opens: the connections, variables,
/// secrets, time and databases the platform offers.
pub trait Opening: Send + Sync {
    /// What reaches `base`, within the run's bounds.
    ///
    /// # Errors
    ///
    /// Where `base` cannot be reached from here, saying why.
    fn connect(&self, base: &str) -> Result<Owned<dyn DynHttp + Send>, String>;

    /// The variable `name`, where the platform sets it and it is not empty.
    fn var(&self, name: &str) -> Option<&str>;

    /// Where secrets are read.
    fn secrets(&self) -> &dyn DynSecrets;

    /// The time.
    fn clock(&self) -> Owned<dyn Clock>;

    /// A nonce no other run or job takes (ADR-0005).
    fn nonce(&self) -> String;

    /// The database at `at`.
    fn sql<'a>(&'a self, at: SqlAt<'a>) -> Local<'a, Result<Owned<dyn DynSql>, String>>;

    /// The key-value store at `at`.
    fn kv<'a>(&'a self, at: KvAt<'a>) -> Local<'a, Result<Owned<dyn DynKv>, String>>;

    /// The secret `name`, which the store needs as `why` says.
    fn secret<'a>(&'a self, name: &'a str, why: &'a str) -> Local<'a, Result<Secret, String>> {
        Box::pin(async move {
            self.secrets()
                .secret_boxed(name)
                .await
                .map_err(|_| format!("{name} is not set; {why}"))
        })
    }

    /// The repository a store names, or else the one the run is for.
    ///
    /// # Errors
    ///
    /// Where the store names none and the platform says none.
    fn repository<'a>(&'a self, named: Option<&'a str>) -> Result<&'a str, String> {
        named.or_else(|| self.var(GITHUB_REPOSITORY)).ok_or_else(|| {
            format!("{GITHUB_REPOSITORY} is not set; name the store's repository, or run herald in the repository's CI")
        })
    }
}

/// A store, opened.
pub enum Opened<'a> {
    /// It keeps its records itself.
    Kept(Box<dyn DynStore + 'a>),
    /// It reads the routes' own posts, as each run connects them.
    Posts,
}

/// Why the `kind` store could not be opened.
#[must_use]
pub fn unopened(kind: &str, error: impl fmt::Display) -> String {
    format!("the {kind} store could not be opened: {error}")
}

/// Where a store's database is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlAt<'a> {
    /// A SQLite file.
    Sqlite {
        /// The file.
        file: &'a Path,
        /// How long a transaction waits for another process to let go of it.
        busy: SignedDuration,
    },
    /// A Postgres database, by its connection URL.
    Postgres {
        /// The connection URL.
        url: &'a Secret,
        /// How long making a connection may take.
        connect: SignedDuration,
        /// How long one statement may run before the database cancels it.
        statement: SignedDuration,
    },
    /// A Cloudflare D1 database, by the Worker's binding to it.
    D1(&'a str),
    /// The storage of a Durable Object.
    DurableObject {
        /// The Worker's binding to the object's namespace.
        binding: &'a str,
        /// The object's name.
        object: &'a str,
    },
}

/// Where a store's key-value store is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KvAt<'a> {
    /// Deno KV: the default database, or the one at a path or URL.
    DenoKv(Option<&'a str>),
    /// A Fastly KV Store, by its name.
    FastlyKv {
        /// The KV Store's name.
        store: &'a str,
        /// How long a deleted item's mark is kept before Fastly expires it.
        tombstone: SignedDuration,
    },
}

#[cfg(test)]
pub(crate) mod tests;
