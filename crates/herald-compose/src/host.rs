use std::future::{Future, ready};

use herald_kinds::{KvAt, SqlAt};
use herald_ports::{
    Changed, Clock, Http, Kv, KvError, Outbound, Pair, Proxy, Secrets, Signer, Sql, SqlError,
    Stamped, Statement, Verifier, Versionstamp,
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

    /// What reaches `base`, within `outbound`'s bounds, through its proxy
    /// and trusting its roots.
    ///
    /// # Errors
    ///
    /// Where `base` cannot be reached from here, or this host cannot go
    /// through `outbound`'s proxy or trust its roots, saying why.
    fn connect(&self, base: &str, outbound: &Outbound) -> Result<Self::Http, String>;

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

/// The database, or key-value store, of a host that offers none: every
/// statement and key is refused.
#[derive(Debug)]
pub struct Unoffered(());

const UNOFFERED: &str = "this host offers none";

fn unreached<T: Send>() -> impl Future<Output = Result<T, KvError>> + Send {
    ready(Err(KvError::unavailable(UNOFFERED)))
}

impl Sql for Unoffered {
    fn transaction(
        &self,
        _: Vec<Statement>,
    ) -> impl Future<Output = Result<Vec<Changed>, SqlError>> + Send {
        ready(Err(SqlError::unavailable(UNOFFERED)))
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

/// `outbound`, where `host` can keep it: refused where it names a proxy or
/// certificate authorities `host` cannot use. The proxy's URL is never
/// repeated, as it may hold its credentials.
///
/// # Errors
///
/// `outbound` names a proxy or roots, saying which and what to write.
pub fn plain(outbound: &Outbound, host: &str) -> Result<(), String> {
    if matches!(outbound.proxy, Proxy::At(_)) {
        return Err(format!(
            "{host} cannot send herald's requests through the proxy [http] proxy names; set it to \"environment\" or \"none\""
        ));
    }
    if let Some(roots) = &outbound.roots {
        return Err(format!(
            "{host} cannot trust the certificate authorities in {}; remove [http] ca_file",
            roots.file
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_proxy_or_roots_a_host_cannot_use_are_refused_without_the_proxy_s_url() {
        assert_eq!(plain(&Outbound::default(), "this host"), Ok(()));
        let proxied = Outbound {
            proxy: Proxy::At("https://user:secret@proxy.example".to_owned()),
            ..Outbound::default()
        };
        let refused = plain(&proxied, "this host").unwrap_err();
        assert!(refused.starts_with("this host cannot send"), "{refused}");
        assert!(!refused.contains("secret"), "{refused}");
        let rooted = Outbound {
            roots: Some(herald_ports::Roots {
                file: "ca.pem".to_owned(),
                pem: String::new(),
            }),
            ..Outbound::default()
        };
        assert_eq!(
            plain(&rooted, "this host"),
            Err("this host cannot trust the certificate authorities in ca.pem; remove [http] ca_file".to_owned())
        );
        let unproxied = Outbound {
            proxy: Proxy::None,
            ..Outbound::default()
        };
        assert_eq!(plain(&unproxied, "this host"), Ok(()));
    }

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
