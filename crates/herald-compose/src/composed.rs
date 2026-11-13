use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use herald_config::{Config, Uses};
use herald_flows::{
    Admission, Connected, Housekept, Inbound, Job, Keys, Place, Ports, Room, Round, Running,
    Serving, Sessions, Stop, Swept, Taker, Worked, answer, busy, housekeep, poll, prepare, refusal,
    unworked, work,
};
use herald_kinds::Opened;
use herald_ports::{Keyring, PUBLIC, Sockets, Store as Keeps, Timer};
use herald_records::{Kept, Memory, MemoryKeyring};

use crate::answer::{Answer, Left};
use crate::capped::Capped;
use crate::host::Host;
use crate::hosting::Hosting;
use crate::records::{Opener, Records, Served, reached};

/// The variable that bounds a request's whole time, in milliseconds, in
/// place of the config's `[http] total`.
const TIMEOUT_MS: &str = "HERALD_TIMEOUT_MS";

/// herald composed on a host: the config, the store it names, and every
/// route, source and destination connected, ready to answer requests, do
/// the work they leave, and run each scheduled round.
pub struct Composed<'o, H: Host + 'o> {
    host: Arc<H>,
    config: Config,
    records: Records<'o>,
    connected: Connected<H::Http>,
    public: Capped<H::Http>,
    memory: MemoryKeyring,
    room: Room,
    polling: AtomicBool,
    housekeeping: AtomicBool,
    run: String,
}

/// Where a composed herald keeps its records.
enum Keeping {
    /// In the store the config names, of a kind `hosting` keeps records in.
    Stored(Hosting),
    /// In memory, for a run that keeps no record.
    Remembered,
}

impl<'o, H: Host + 'o> Composed<'o, H>
where
    H: Send,
    H::Http: Send + 'static,
    H::Clock: Send + 'static,
    H::Sql: 'static,
    H::Kv: 'static,
{
    /// `config` composed on `host`, as `hosting` describes it, keeping its
    /// records in a store of a kind it keeps records in, reading the secrets
    /// a run that `uses` the config reads, and taking leases under the run
    /// name `run`.
    ///
    /// # Errors
    ///
    /// Where the store, a secret or a base URL cannot be reached, saying why.
    pub async fn new(
        host: H,
        config: Config,
        hosting: Hosting,
        uses: Uses,
        run: impl Into<String>,
    ) -> Result<Self, String> {
        Self::kept(host, config, uses, run.into(), Keeping::Stored(hosting)).await
    }

    /// `config` composed on `host` as [`Composed::new`] composes it, its
    /// records kept in memory for a run that keeps no record.
    ///
    /// # Errors
    ///
    /// Where a secret or a base URL cannot be reached, saying why.
    pub async fn dry(
        host: H,
        config: Config,
        uses: Uses,
        run: impl Into<String>,
    ) -> Result<Self, String> {
        Self::kept(host, config, uses, run.into(), Keeping::Remembered).await
    }

    async fn kept(
        host: H,
        config: Config,
        uses: Uses,
        run: String,
        keeping: Keeping,
    ) -> Result<Self, String> {
        let host = Arc::new(host);
        let outbound = config.http.clone().timed(host.var(TIMEOUT_MS));
        let prepared = prepare(&config, host.secrets(), host.signer(), uses)
            .await
            .map_err(|error| error.to_string())?;
        let records = match keeping {
            Keeping::Stored(hosting) => {
                let opener = Opener::new(Arc::clone(&host), outbound.clone());
                reached(Arc::new(opener), &config.store, &hosting.stores()).await?
            }
            Keeping::Remembered => {
                Records::new(Opened::Kept(Box::new(Kept::new(Memory::default()))))
            }
        };
        let connected = prepared.connect(|base| host.connect(base, &outbound))?;
        let public = Capped::new(
            host.connect(PUBLIC, &outbound.public())?,
            config.server.fetches,
        );
        let memory = MemoryKeyring::new(config.server.keys_most);
        let room = Room::new(config.server.jobs);
        Ok(Self {
            host,
            config,
            records,
            connected,
            public,
            memory,
            room,
            polling: AtomicBool::new(false),
            housekeeping: AtomicBool::new(false),
            run,
        })
    }
}

impl<'o, H: Host + 'o> Composed<'o, H> {
    /// The host composed on.
    pub fn host(&self) -> &H {
        &self.host
    }

    /// The config composed.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Every route, source and destination, connected.
    pub fn connected(&self) -> &Connected<H::Http> {
        &self.connected
    }

    /// The reply to `inbound`, and the work it leaves, its delivery admitted
    /// as `admission` says; a round it asks for with the round secret is
    /// run.
    pub async fn answer(&self, inbound: &Inbound<'_>, admission: Admission) -> Answer {
        if let Some(round) = Round::named(inbound.method, inbound.path) {
            return self.round(round, inbound.header("authorization")).await;
        }
        if inbound.body.len() > self.config.server.body {
            return refusal(
                413,
                format!(
                    "its body is larger than [server] body, {} bytes",
                    self.config.server.body
                ),
            )
            .into();
        }
        let answering = Answering {
            inbound,
            room: &self.room,
        };
        self.through(admission, answering).await
    }

    /// A place for one more piece of work, where `[server] jobs` leaves one.
    #[must_use]
    pub fn place(&self) -> Option<Place> {
        self.room.place()
    }

    /// The work `left` done, its place given up after.
    pub async fn work(&self, left: Left) -> Worked {
        let Left { job, place } = left;
        let worked = self.through(Admission::Once, Working(job)).await;
        drop(place);
        worked
    }

    /// Every source polled, where no poll is running; none where one is.
    pub async fn poll(&self) -> Vec<Swept> {
        let Some(_running) = Running::start(&self.polling) else {
            return Vec::new();
        };
        self.through(Admission::Once, Polling).await
    }

    /// The store's housekeeping done, and the keys read longest ago
    /// forgotten, where no housekeeping is running; nothing where it is.
    ///
    /// # Errors
    ///
    /// Where housekeeping stopped, saying why.
    pub async fn housekeep(&self) -> Result<Housekept, Stop> {
        let Some(_running) = Running::start(&self.housekeeping) else {
            return Ok(Housekept::default());
        };
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

    /// `work` done through everything composed.
    pub async fn run<W: Through<H>>(&self, work: W) -> W::Done {
        self.through(Admission::Once, work).await
    }

    async fn through<W: Through<H>>(&self, admission: Admission, work: W) -> W::Done {
        let through = Keying {
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
pub type ServingOn<'a, H, S, K> = Serving<
    'a,
    S,
    <H as Host>::Clock,
    <H as Host>::Signer,
    <H as Host>::Verifier,
    <H as Host>::Http,
    Capped<<H as Host>::Http>,
    K,
>;

/// Work done through a composed herald: over its store, with the keys it
/// checks activities with, its leases taken under a run name of its own.
pub trait Through<H: Host> {
    /// What the work did.
    type Done;

    /// The work done through `serving`, taking leases under `run`.
    fn through<S: Keeps, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        run: &str,
    ) -> impl Future<Output = Self::Done>;
}

struct Keying<'c, H: Host, W> {
    composed: &'c Composed<'c, H>,
    admission: Admission,
    work: W,
}

impl<H: Host, W: Through<H>> Served for Keying<'_, H, W> {
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

impl<H: Host, W: Through<H>> Keying<'_, H, W> {
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
            reread: composed.config.server.keys_reread,
        };
        let serving = Serving {
            config: &composed.config,
            connected: &composed.connected,
            ports: &ports,
            verifier: host.verifier(),
            public: &composed.public,
            keys: &keys,
            webhooks: composed.connected.webhooks(),
            admission: self.admission,
        };
        let run = format!("{}/{}", composed.run, host.nonce());
        self.work.through(&serving, &run).await
    }
}

struct Answering<'i, 'a> {
    inbound: &'i Inbound<'a>,
    room: &'i Room,
}

impl<H: Host> Through<H> for Answering<'_, '_> {
    type Done = Answer;

    async fn through<S: Keeps, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        _: &str,
    ) -> Answer {
        let mut answered = answer(serving, self.inbound).await;
        let Some(job) = answered.job.take() else {
            return answered.into();
        };
        let Some(place) = self.room.place() else {
            unworked(serving, &job).await;
            return busy(serving.config.server.busy_retry).into();
        };
        Answer {
            left: Some(Left { job, place }),
            ..answered.into()
        }
    }
}

struct Working(Job);

impl<H: Host> Through<H> for Working {
    type Done = Worked;

    async fn through<S: Keeps, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        run: &str,
    ) -> Worked {
        work(serving, self.0, run).await
    }
}

struct Polling;

impl<H: Host> Through<H> for Polling {
    type Done = Vec<Swept>;

    async fn through<S: Keeps, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
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

impl<H: Host, So: Sockets, T: Taker, Ti: Timer> Through<H> for Holding<'_, So, T, Ti> {
    type Done = ();

    async fn through<S: Keeps, K: Keyring>(self, serving: &ServingOn<'_, H, S, K>, _: &str) {
        Sessions::new(serving, self.taker, self.timer)
            .held(self.sockets)
            .await;
    }
}

struct Housekeeping;

impl<H: Host> Through<H> for Housekeeping {
    type Done = Result<Housekept, Stop>;

    async fn through<S: Keeps, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        _: &str,
    ) -> Result<Housekept, Stop> {
        let ports = serving.ports;
        housekeep(
            serving.connected,
            ports.store,
            ports.clock,
            serving.keys,
            &serving.config.retention,
        )
        .await
    }
}
