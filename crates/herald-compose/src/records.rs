use std::future::Future;
use std::sync::Arc;

use herald_config::ConfiguredStore;
use herald_core::Attempts;
use herald_flows::{Connected, unserved};
use herald_kinds::{KvAt, Local, Opened, Opening, SqlAt};
use herald_ports::{
    Clock, DynHttp, DynKv, DynSecrets, DynSql, DynStore, Erased, Http, Keyring, Outbound, Owned,
    RecordOptions, Signer, Store,
};

use crate::host::Host;

pub use herald_kinds::{GITHUB_REPOSITORY, GITHUB_TOKEN};

/// Where a run keeps its records, as the config names it, opened.
pub struct OpenedRecords<'o>(Opened<'o>);

impl<'o> OpenedRecords<'o> {
    /// The records `opened` keeps.
    #[must_use]
    pub const fn new(opened: Opened<'o>) -> Self {
        Self(opened)
    }

    /// Whether it reads the routes' own posts rather than keeping records.
    #[must_use]
    pub const fn reads_posts(&self) -> bool {
        matches!(self.0, Opened::Posts)
    }

    /// `work` done over the store, or over the routes' posts, signed by
    /// `signer`, where they keep the records.
    pub async fn serve<W: Served, H: Http + Clone, G: Signer + Clone>(
        &self,
        connected: &Connected<H>,
        signer: &G,
        work: W,
    ) -> W::Done {
        match &self.0 {
            Opened::Kept(store) => work.over(&Erased(&**store as &dyn DynStore)).await,
            Opened::Posts => work.over(&connected.marked(signer.clone())).await,
        }
    }
}

/// Work done over whichever store the config names.
pub trait Served {
    /// What the work did.
    type Done;

    /// The work done over `store`.
    fn over<S: Store + Keyring>(self, store: &S) -> impl Future<Output = Self::Done>;
}

/// A host, as the stores it opens see it: its connections bound by
/// `outbound`.
pub struct Opener<H> {
    host: Arc<H>,
    outbound: Outbound,
}

impl<H: Host> Opener<H> {
    /// `host`, opening stores whose connections keep within `outbound`.
    #[must_use]
    pub const fn new(host: Arc<H>, outbound: Outbound) -> Self {
        Self { host, outbound }
    }
}

impl<H> Opening for Opener<H>
where
    H: Host + Send,
    H::Http: Send + 'static,
    H::Clock: Send + 'static,
    H::Sql: 'static,
    H::Kv: 'static,
{
    fn connect(&self, base: &str) -> Result<Owned<dyn DynHttp + Send>, String> {
        let http = self.host.connect(base, &self.outbound)?;
        Ok(Owned::new(Box::new(http)))
    }

    fn var(&self, name: &str) -> Option<&str> {
        self.host.var(name)
    }

    fn secrets(&self) -> &dyn DynSecrets {
        self.host.secrets()
    }

    fn clock(&self) -> Owned<dyn Clock> {
        Owned::new(Box::new(self.host.clock().clone()))
    }

    fn nonce(&self) -> String {
        self.host.nonce()
    }

    fn sql<'a>(&'a self, at: SqlAt<'a>) -> Local<'a, Result<Owned<dyn DynSql>, String>> {
        Box::pin(async move {
            let sql = self.host.sql(at).await?;
            Ok(Owned::new(Box::new(sql) as Box<dyn DynSql>))
        })
    }

    fn kv<'a>(&'a self, at: KvAt<'a>) -> Local<'a, Result<Owned<dyn DynKv>, String>> {
        Box::pin(async move {
            let kv = self.host.kv(at).await?;
            Ok(Owned::new(Box::new(kv) as Box<dyn DynKv>))
        })
    }
}

/// The store `store` names, opened through `opening`, where it is one of
/// the kinds in `stores`, trying a write that lost a race `attempts` times.
///
/// # Errors
///
/// Where the store is not one of `stores`, or cannot be reached, saying why.
pub async fn reached<'o>(
    opening: Arc<dyn Opening + 'o>,
    store: &ConfiguredStore,
    attempts: Attempts,
    stores: &[&str],
) -> Result<OpenedRecords<'o>, String> {
    if !stores.contains(&store.kind()) {
        return Err(unserved(stores));
    }
    let seal = match store.key() {
        Some(key) => Some(
            opening
                .secret(&key.secret, "the store seals its records with it")
                .await?,
        ),
        None => None,
    };
    store
        .open(opening, RecordOptions { seal, attempts })
        .await
        .map(OpenedRecords::new)
}
