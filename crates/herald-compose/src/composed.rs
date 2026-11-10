use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use herald_config::Config;
use herald_core::Retention;
use herald_flows::{
    Admission, Connected, Housekept, Inbound, Job, Keys, Place, Ports, Room, Round, Running,
    Serving, Sessions, Stop, Swept, Taker, Webhooks, Worked, answer, busy, housekeep, poll,
    prepare, refusal, unworked, webhooks, work,
};
use herald_ports::{Clock as _, Keyring, PUBLIC, Sockets, Store as Keeps, Timer};
use herald_records::MemoryKeyring;
use jiff::Timestamp;

use crate::answer::{Answer, Left};
use crate::capped::Capped;
use crate::host::Host;
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
    webhooks: Webhooks,
    public: Capped<H::Http>,
    memory: MemoryKeyring,
    room: Room,
    polling: AtomicBool,
    housekeeping: AtomicBool,
    run: String,
}

impl<'o, H: Host + 'o> Composed<'o, H> {
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
    ) -> Result<Self, String>
    where
        H: Send,
        H::Http: Send + 'static,
        H::Clock: Send + 'static,
        H::Sql: 'static,
        H::Kv: 'static,
    {
        let host = Arc::new(host);
        let outbound = config.http.clone().timed(host.var(TIMEOUT_MS));
        let opener = Opener::new(Arc::clone(&host), outbound.clone());
        let records = reached(Arc::new(opener), &config.store, stores).await?;
        let prepared = prepare(&config, host.secrets(), host.signer())
            .await
            .map_err(|error| error.to_string())?
            .answering(host.secrets())
            .await
            .map_err(|error| error.to_string())?;
        let connected = prepared.connect(|base| host.connect(base, &outbound))?;
        let webhooks = webhooks(&config, host.secrets())
            .await
            .map_err(|error| error.to_string())?;
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
            webhooks,
            public,
            memory,
            room,
            polling: AtomicBool::new(false),
            housekeeping: AtomicBool::new(false),
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
        self.through(Admission::Once, Housekeeping(self.config.retention))
            .await
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
    composed: &'c Composed<'c, H>,
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
            reread: composed.config.server.keys_reread,
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
        let run = format!("{}/{}", composed.run, host.nonce());
        self.work.keyed(&serving, &run).await
    }
}

struct Answering<'i, 'a> {
    inbound: &'i Inbound<'a>,
    room: &'i Room,
}

impl<H: Host> Keyed<H> for Answering<'_, '_> {
    type Done = Answer;

    async fn keyed<S: Keeps, K: Keyring>(self, serving: &Hosted<'_, H, S, K>, _: &str) -> Answer {
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

struct Housekeeping(Retention);

impl<H: Host> Keyed<H> for Housekeeping {
    type Done = Result<Housekept, Stop>;

    async fn keyed<S: Keeps, K: Keyring>(
        self,
        serving: &Hosted<'_, H, S, K>,
        _: &str,
    ) -> Result<Housekept, Stop> {
        let ports = serving.ports;
        let before = ports
            .clock
            .now()
            .checked_sub(serving.keys.refreshed)
            .unwrap_or(Timestamp::MIN);
        let _ = serving.keys.keyring.forget_before(before).await;
        housekeep(serving.connected, ports.store, ports.clock, &self.0).await
    }
}
