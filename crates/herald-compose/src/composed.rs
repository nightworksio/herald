use std::future::Future;

use herald_config::Config;
use herald_flows::{
    Admission, Answered, Connected, Inbound, Job, Kept, Keys, Ports, Serving, Sessions, Stop,
    Swept, Taker, Webhooks, Worked, answer, housekeep, poll, prepare, webhooks, work,
};
use herald_ports::{Clock as _, Keyring, PUBLIC, Sockets, Store as Keeps, Timer};
use herald_records::MemoryKeyring;
use jiff::Timestamp;

use crate::capped::Capped;
use crate::host::Host;
use crate::records::{Records, Served, reached};

/// herald composed on a host: the config, the store it names, and every
/// route, source and destination connected, ready to answer requests, do
/// the work they leave, and run each scheduled round.
pub struct Composed<H: Host> {
    host: H,
    config: Config,
    records: Records<H>,
    connected: Connected<H::Http>,
    webhooks: Webhooks,
    public: Capped<H::Http>,
    memory: MemoryKeyring,
    run: String,
}

impl<H: Host> Composed<H> {
    /// `config` composed on `host`, keeping its records in a store of one
    /// of the kinds in `stores`, and taking leases under the run name `run`.
    ///
    /// # Errors
    ///
    /// Where the store, a secret or a base URL cannot be reached, saying why.
    pub async fn new(
        host: H,
        config: Config,
        stores: &[&str],
        run: impl Into<String>,
    ) -> Result<Self, String> {
        let records = reached(&host, &config.store, stores).await?;
        let prepared = prepare(&config, host.secrets(), host.signer())
            .await
            .map_err(|error| error.to_string())?
            .answering(host.secrets())
            .await
            .map_err(|error| error.to_string())?;
        let connected = prepared.connect(|base| host.connect(base))?;
        let webhooks = webhooks(&config, host.secrets())
            .await
            .map_err(|error| error.to_string())?;
        let public = Capped::new(host.connect(PUBLIC)?, config.server.fetches);
        let memory = MemoryKeyring::new(config.server.keys_most);
        Ok(Self {
            host,
            config,
            records,
            connected,
            webhooks,
            public,
            memory,
            run: run.into(),
        })
    }

    /// The host composed on.
    pub fn host(&self) -> &H {
        &self.host
    }

    /// The config composed.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The reply to `inbound`, and the work it leaves, its delivery admitted
    /// as `admission` says.
    pub async fn answer(&self, inbound: &Inbound<'_>, admission: Admission) -> Answered {
        self.through(admission, Answering(inbound)).await
    }

    /// `job` done.
    pub async fn work(&self, job: Job) -> Worked {
        self.through(Admission::Once, Working(job)).await
    }

    /// Every source polled.
    pub async fn poll(&self) -> Vec<Swept> {
        self.through(Admission::Once, Polling).await
    }

    /// The store's housekeeping done, and the keys read longest ago
    /// forgotten.
    ///
    /// # Errors
    ///
    /// Where housekeeping stopped, saying why.
    pub async fn housekeep(&self) -> Result<Vec<Kept>, Stop> {
        self.through(Admission::Once, Housekeeping).await
    }

    /// Every session the destinations hold, opened through `sockets`, each
    /// command's work handed to `taker`, waiting with `timer`, until each
    /// stops.
    pub async fn hold<So: Sockets, T: Taker, Ti: Timer>(
        &self,
        sockets: &So,
        taker: &T,
        timer: &Ti,
    ) {
        let holding = Holding {
            sockets,
            taker,
            timer,
        };
        self.through(Admission::Once, holding).await;
    }

    async fn through<W: Keyed<H>>(&self, admission: Admission, work: W) -> W::Done {
        let through = Through {
            composed: self,
            admission,
            work,
        };
        self.records
            .serve(&self.connected, self.host.signer(), through)
            .await
    }
}

/// What serves on `H`, over the store `S` and the keys in `K`.
type Hosted<'a, H, S, K> = Serving<
    'a,
    S,
    <H as Host>::Clock,
    <H as Host>::Signer,
    <H as Host>::Verifier,
    <H as Host>::Http,
    Capped<<H as Host>::Http>,
    K,
>;

/// Work done over a store and the keys it checks activities with.
trait Keyed<H: Host> {
    type Done;

    fn keyed<S: Keeps, K: Keyring>(
        self,
        serving: &Hosted<'_, H, S, K>,
        run: &str,
    ) -> impl Future<Output = Self::Done>;
}

struct Through<'c, H: Host, W> {
    composed: &'c Composed<H>,
    admission: Admission,
    work: W,
}

impl<H: Host, W: Keyed<H>> Served for Through<'_, H, W> {
    type Done = W::Done;

    async fn over<S: Keeps + Keyring>(self, store: &S) -> W::Done {
        let composed = self.composed;
        if composed.config.server.keys_in_store() {
            self.keyed(store, store).await
        } else {
            self.keyed(store, &composed.memory).await
        }
    }
}

impl<H: Host, W: Keyed<H>> Through<'_, H, W> {
    async fn keyed<S: Keeps, K: Keyring>(self, store: &S, keyring: &K) -> W::Done {
        let composed = self.composed;
        let host = &composed.host;
        let ports = Ports {
            store,
            clock: host.clock(),
            signer: host.signer(),
        };
        let keys = Keys {
            keyring,
            refreshed: composed.config.server.keys_refreshed,
        };
        let serving = Serving {
            config: &composed.config,
            connected: &composed.connected,
            ports: &ports,
            verifier: host.verifier(),
            public: &composed.public,
            keys: &keys,
            webhooks: &composed.webhooks,
            admission: self.admission,
        };
        self.work.keyed(&serving, &composed.run).await
    }
}

struct Answering<'i, 'a>(&'i Inbound<'a>);

impl<H: Host> Keyed<H> for Answering<'_, '_> {
    type Done = Answered;

    async fn keyed<S: Keeps, K: Keyring>(self, serving: &Hosted<'_, H, S, K>, _: &str) -> Answered {
        answer(serving, self.0).await
    }
}

struct Working(Job);

impl<H: Host> Keyed<H> for Working {
    type Done = Worked;

    async fn keyed<S: Keeps, K: Keyring>(self, serving: &Hosted<'_, H, S, K>, run: &str) -> Worked {
        work(serving, self.0, run).await
    }
}

struct Polling;

impl<H: Host> Keyed<H> for Polling {
    type Done = Vec<Swept>;

    async fn keyed<S: Keeps, K: Keyring>(
        self,
        serving: &Hosted<'_, H, S, K>,
        run: &str,
    ) -> Vec<Swept> {
        poll(serving.config, serving.connected, serving.ports, run).await
    }
}

struct Holding<'h, So, T, Ti> {
    sockets: &'h So,
    taker: &'h T,
    timer: &'h Ti,
}

impl<H: Host, So: Sockets, T: Taker, Ti: Timer> Keyed<H> for Holding<'_, So, T, Ti> {
    type Done = ();

    async fn keyed<S: Keeps, K: Keyring>(self, serving: &Hosted<'_, H, S, K>, _: &str) {
        Sessions::new(serving, self.taker, self.timer)
            .held(self.sockets)
            .await;
    }
}

struct Housekeeping;

impl<H: Host> Keyed<H> for Housekeeping {
    type Done = Result<Vec<Kept>, Stop>;

    async fn keyed<S: Keeps, K: Keyring>(
        self,
        serving: &Hosted<'_, H, S, K>,
        _: &str,
    ) -> Result<Vec<Kept>, Stop> {
        let ports = serving.ports;
        let kept = housekeep(serving.connected, ports.store, ports.clock).await;
        let before = ports
            .clock
            .now()
            .checked_sub(serving.keys.refreshed)
            .unwrap_or(Timestamp::MIN);
        let _ = serving.keys.keyring.forget_before(before).await;
        kept
    }
}
