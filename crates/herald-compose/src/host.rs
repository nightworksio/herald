use std::future::{Future, ready};
use std::path::Path;

use herald_ports::{
    Changed, Clock, Http, Kv, KvError, Pair, Secret, Secrets, Signer, Sql, SqlError, Stamped,
    Statement, Verifier, Versionstamp,
};

/// What a platform gives herald: its ports, its variables, and the
/// databases it offers.
pub trait Host: Sync {
    /// What reaches one base URL.
    type Http: Http + Clone;
    /// Where secrets are read.
    type Secrets: Secrets;
    /// The time.
    type Clock: Clock + Clone;
    /// What signs.
    type Signer: Signer + Clone;
    /// What verifies.
    type Verifier: Verifier;
    /// A database the platform offers.
    type Sql: Sql;
    /// A key-value store the platform offers.
    type Kv: Kv;

    /// What reaches `base`.
    ///
    /// # Errors
    ///
    /// Where `base` cannot be reached from here, saying why.
    fn connect(&self, base: &str) -> Result<Self::Http, String>;

    /// The variable `name`, where the platform sets it and it is not empty.
    fn var(&self, name: &str) -> Option<&str>;

    /// Where secrets are read.
    fn secrets(&self) -> &Self::Secrets;

    /// The time.
    fn clock(&self) -> &Self::Clock;

    /// What signs.
    fn signer(&self) -> &Self::Signer;

    /// What verifies.
    fn verifier(&self) -> &Self::Verifier;

    /// The database at `at`.
    ///
    /// # Errors
    ///
    /// Where the platform offers no such database, or it cannot be opened,
    /// saying why.
    fn sql(&self, at: SqlAt<'_>) -> impl Future<Output = Result<Self::Sql, String>>;

    /// The key-value store at `at`.
    ///
    /// # Errors
    ///
    /// Where the platform offers no such store, or it cannot be opened,
    /// saying why.
    fn kv(&self, at: KvAt<'_>) -> impl Future<Output = Result<Self::Kv, String>>;

    /// A nonce no other run or job takes, which keeps each job's name for
    /// its leases its own (ADR-0005).
    fn nonce(&self) -> String;
}

/// Where a store's database is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlAt<'a> {
    /// A SQLite file.
    Sqlite(&'a Path),
    /// A Postgres database, by its connection URL.
    Postgres(&'a Secret),
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
    FastlyKv(&'a str),
}

/// The database, or key-value store, of a host that offers none: every
/// statement and key is refused.
#[derive(Debug)]
pub struct Unoffered(());

const UNOFFERED: &str = "this host offers none";

fn unreached<T: Send>() -> impl Future<Output = Result<T, KvError>> + Send {
    ready(Err(KvError::Unavailable {
        reason: UNOFFERED.to_owned(),
    }))
}

impl Sql for Unoffered {
    fn transaction(
        &self,
        _: Vec<Statement>,
    ) -> impl Future<Output = Result<Vec<Changed>, SqlError>> + Send {
        ready(Err(SqlError::Unavailable {
            reason: UNOFFERED.to_owned(),
        }))
    }
}

impl Kv for Unoffered {
    fn get(&self, _: &[String]) -> impl Future<Output = Result<Option<Stamped>, KvError>> + Send {
        unreached()
    }

    fn set_if(
        &self,
        _: &[String],
        _: Vec<u8>,
        _: Option<Versionstamp>,
    ) -> impl Future<Output = Result<Option<Versionstamp>, KvError>> + Send {
        unreached()
    }

    fn delete_if(
        &self,
        _: &[String],
        _: Versionstamp,
    ) -> impl Future<Output = Result<bool, KvError>> + Send {
        unreached()
    }

    fn list(&self, _: &[String]) -> impl Future<Output = Result<Vec<Pair>, KvError>> + Send {
        unreached()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_host_does_not_offer_refuses_everything() {
        let none = Unoffered(());
        let key = ["k".to_owned()];
        let stamp = Versionstamp("1".to_owned());
        let sql = pollster::block_on(none.transaction(Vec::new()));
        assert_eq!(
            sql,
            Err(SqlError::Unavailable {
                reason: UNOFFERED.to_owned()
            })
        );
        let kv = KvError::Unavailable {
            reason: UNOFFERED.to_owned(),
        };
        assert_eq!(pollster::block_on(none.get(&key)), Err(kv.clone()));
        assert_eq!(
            pollster::block_on(none.set_if(&key, Vec::new(), None)),
            Err(kv.clone())
        );
        assert_eq!(
            pollster::block_on(none.delete_if(&key, stamp)),
            Err(kv.clone())
        );
        assert_eq!(pollster::block_on(none.list(&key)), Err(kv));
        assert!(format!("{none:?}").starts_with("Unoffered"));
    }
}
