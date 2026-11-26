use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use herald_config::{Config, Uses};
use herald_core::{Board, Printed, RouteId, status};
use herald_flows::{
    Admission, Budget, Connected, Handing, Housekept, Inbound, Job, Keys, Orphaned, Place, Ports,
    Publishing, Requests, Room, Round, Running, Serving, Sessions, Stop, Swept, Taker, Worked,
    answer, housekeep_announcing, orphans, orphans_cleared, poll, prepare, publish, queue, refusal,
    report, unplaced, work,
};
use herald_kinds::{GITHUB_REPOSITORY, Opened};
use herald_ports::{Base, Clock as _, Keyring, Sockets, Store, Timer};
use herald_records::{Memory, MemoryKeyring, RecordStore};
use jiff::{SignedDuration, Timestamp};

use crate::ASKS_MODELS;
use crate::answer::{Answer, Left};
use crate::capped::Capped;
use crate::counted::{Counted, binding};
use crate::host::Host;
use crate::hosting::Hosting;
use crate::records::{OpenedRecords, Opener, Served, reached};

mod merging;
mod rerunning;

/// The variable that bounds a request's whole time, in milliseconds, in
/// place of the config's `[http] total`.
const TIMEOUT_MS: &str = "HERALD_TIMEOUT_MS";

/// herald composed on a host: the config, the store it names, and every
/// route, source and destination connected, ready to answer requests, do
/// the work they leave, and run each scheduled round.
pub struct Composed<'o, H: Host + 'o> {
    host: Arc<H>,
    config: Config,
    records: OpenedRecords<'o>,
    connected: Connected<Counted<H::Http>>,
    public: Capped<Counted<H::Http>>,
    made: Arc<AtomicUsize>,
    after: Option<SignedDuration>,
    requests: Option<NonZeroUsize>,
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
        let made = Arc::new(AtomicUsize::new(0));
        let outbound = config.http.clone().timed(host.var(TIMEOUT_MS));
        let work = config.server.work;
        let (after, requests) = match keeping {
            Keeping::Stored(hosting) => (
                work.after.or(hosting.after()),
                work.requests.or(hosting.requests()),
            ),
            Keeping::Remembered => (work.after, work.requests),
        };
        if let Some(after) = after
            && after <= work.spare
        {
            return Err(format!(
                "[server.work] spare, {:#}, leaves no time for a send in the {after:#} the work may run after its answer; make it shorter",
                work.spare
            ));
        }
        if let Some(most) = requests
            && most.get() <= work.spare_requests
        {
            return Err(format!(
                "[server.work] spare_requests, {}, leaves no room for a send in the {most} outbound requests a request may make; make it fewer",
                work.spare_requests
            ));
        }
        let prepared = prepare(&config, host.secrets(), host.signer(), uses)
            .await
            .map_err(|error| error.to_string())?
            .asking(ASKS_MODELS)
            .in_repository(host.var(GITHUB_REPOSITORY));
        let records = match keeping {
            Keeping::Stored(hosting) => {
                let opener = Opener::new(Arc::clone(&host), outbound.clone(), Arc::clone(&made));
                reached(
                    Arc::new(opener),
                    &config.store,
                    config.attempts,
                    &hosting.stores(),
                )
                .await?
            }
            Keeping::Remembered => {
                OpenedRecords::new(Opened::Kept(Box::new(RecordStore::new(Memory::default()))))
            }
        };
        let counted = |http| Counted::new(http, Arc::clone(&made));
        let connected = prepared
            .connect(|base| host.connect(base, &outbound).map(counted))?
            .bind(binding(&*host, &made))?;
        let public = Capped::new(
            counted(host.connect(Base::Public, &outbound.public())?),
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
            made,
            after,
            requests,
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
    pub fn connected(&self) -> &Connected<Counted<H::Http>> {
        &self.connected
    }

    /// Where a run starting now counts its outbound requests from.
    fn started(&self) -> Started {
        Started {
            from: self.made.load(Ordering::Relaxed),
            until: None,
        }
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
                status::CONTENT_TOO_LARGE,
                format!(
                    "its body is larger than [server] body, {} bytes",
                    self.config.server.body
                ),
            )
            .into();
        }
        let started = self.started();
        let answering = Answering {
            inbound,
            room: &self.room,
            from: started.from,
        };
        self.through(admission, started, answering).await
    }

    /// A place for one more piece of work, where `[server] jobs` leaves one.
    #[must_use]
    pub fn place(&self) -> Option<Place> {
        self.room.place()
    }

    /// The work `left` done, its place given up after: no send starts once
    /// the host's limits leave less than their spare, and the next run
    /// sends what is left.
    pub async fn work(&self, left: Left) -> Worked {
        let Left { job, place, from } = left;
        let started = Started {
            from: from.unwrap_or_else(|| self.made.load(Ordering::Relaxed)),
            until: self.after.map(|after| {
                self.host
                    .clock()
                    .now()
                    .checked_add(after)
                    .unwrap_or(Timestamp::MAX)
            }),
        };
        let worked = self.through(Admission::Once, started, Working(job)).await;
        drop(place);
        worked
    }

    /// The board of the view `view`, its sources' open pull requests read
    /// now (ADR-0055), a private or internal repository's rows printed as
    /// `printed` says and its advisories only where the operator asked
    /// (ADR-0067).
    ///
    /// # Errors
    ///
    /// Where the config has no such view, or a source it reads could not
    /// be read.
    pub async fn queue(&self, view: &str, printed: Printed) -> Result<Board, Stop> {
        self.through(Admission::Once, self.started(), Queueing(view, printed))
            .await
    }

    /// Each possibly orphaned part of the living posts of the report
    /// `report`, of the route `route` alone where it names one, by route,
    /// with what the operator names back to clear them; or why there are
    /// none (ADR-0067).
    ///
    /// # Errors
    ///
    /// Where the report holds no such part, or its records cannot be read.
    pub async fn orphans(
        &self,
        report: &str,
        route: Option<&RouteId>,
    ) -> Result<Vec<Orphaned>, String> {
        self.through(Admission::Once, self.started(), Orphans(report, route))
            .await
    }

    /// The possibly orphaned parts of the report `report`'s living post on
    /// the route `route` dropped, as the operator says they were deleted
    /// by hand, where they are still the ones `seen` names (ADR-0067).
    ///
    /// # Errors
    ///
    /// Where the post holds no such part, holds others than `seen` names,
    /// or its records cannot be read or written, saying whether anything
    /// was cleared.
    pub async fn orphans_cleared(
        &self,
        report: &str,
        seen: (&RouteId, &str),
    ) -> Result<Orphaned, String> {
        self.through(Admission::Once, self.started(), Cleared(report, seen))
            .await
    }

    /// Every report whose window began announced, where no poll is
    /// running; none where one is (ADR-0055).
    pub async fn report(&self) -> Vec<Swept> {
        let Some(_running) = Running::start(&self.polling) else {
            return Vec::new();
        };
        self.through(Admission::Once, self.started(), Reporting)
            .await
    }

    /// Every source polled, then every report due announced, where no poll
    /// is running; none where one is.
    pub async fn poll(&self) -> Vec<Swept> {
        let Some(_running) = Running::start(&self.polling) else {
            return Vec::new();
        };
        self.through(Admission::Once, self.started(), Polling).await
    }

    /// The static signed log `[federation.pages]` names written where it
    /// is due, once the run's feeds changed (ADR-0060); nothing where the
    /// config names none.
    ///
    /// # Errors
    ///
    /// Where the writing stopped, saying why.
    pub async fn publish(&self) -> Result<Option<Publishing>, Stop> {
        self.through(Admission::Once, self.started(), Logging).await
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
        self.through(Admission::Once, self.started(), Housekeeping)
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
        self.through(Admission::Once, self.started(), holding).await;
    }

    /// `work` done through everything composed.
    pub async fn run<W: Through<H>>(&self, work: W) -> W::Done {
        self.through(Admission::Once, self.started(), work).await
    }

    async fn through<W: Through<H>>(
        &self,
        admission: Admission,
        started: Started,
        work: W,
    ) -> W::Done {
        self.stepping(admission, started, work, None).await
    }

    /// `work` done as [`Composed::through`] does it, the merge queues
    /// whose steps `herald server` takes stepped through `merging` where it
    /// is some.
    async fn stepping<W: Through<H>>(
        &self,
        admission: Admission,
        started: Started,
        work: W,
        merging: Option<&Handing<'_, Counted<H::Http>>>,
    ) -> W::Done {
        let through = Keying {
            composed: self,
            admission,
            started,
            work,
            merging,
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
    Counted<<H as Host>::Http>,
    Capped<Counted<<H as Host>::Http>>,
    K,
>;

/// Where a run counts its outbound requests from, and when its host stops
/// it, where it does.
#[derive(Debug, Clone, Copy)]
struct Started {
    from: usize,
    until: Option<Timestamp>,
}

/// Work done through a composed herald: over its store, with the keys it
/// checks activities with, its leases taken under a run name of its own.
pub trait Through<H: Host> {
    /// What the work did.
    type Done;

    /// The work done through `serving`, taking leases under `run`.
    fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        run: &str,
    ) -> impl Future<Output = Self::Done>;
}

struct Keying<'c, 'm, H: Host, W> {
    composed: &'c Composed<'c, H>,
    admission: Admission,
    started: Started,
    work: W,
    merging: Option<&'m Handing<'m, Counted<H::Http>>>,
}

impl<H: Host, W: Through<H>> Served for Keying<'_, '_, H, W> {
    type Done = W::Done;

    async fn over<S: Store + Keyring>(self, store: &S) -> W::Done {
        let composed = self.composed;
        if composed.config.server.keys_in_store() {
            self.with_keyring(store, store).await
        } else {
            self.with_keyring(store, &composed.memory).await
        }
    }
}

impl<H: Host, W: Through<H>> Keying<'_, '_, H, W> {
    /// The work done over `store`, the keys activities are checked with
    /// kept in `keyring`.
    async fn with_keyring<S: Store, K: Keyring>(self, store: &S, keyring: &K) -> W::Done {
        let composed = self.composed;
        let host = &composed.host;
        let work = &composed.config.server.work;
        let ports = Ports {
            store,
            clock: host.clock(),
            signer: host.signer(),
            budget: Budget {
                until: self.started.until,
                spare: work.spare,
                requests: composed.requests.map(|most| Requests {
                    made: &composed.made,
                    from: self.started.from,
                    most: most.get(),
                    spare: work.spare_requests,
                }),
            },
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
            merging: self.merging,
        };
        let run = format!("{}/{}", composed.run, host.nonce());
        self.work.through(&serving, &run).await
    }
}

struct Answering<'i, 'a> {
    inbound: &'i Inbound<'a>,
    room: &'i Room,
    from: usize,
}

impl<H: Host> Through<H> for Answering<'_, '_> {
    type Done = Answer;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        _: &str,
    ) -> Answer {
        let mut answered = answer(serving, self.inbound).await;
        let Some(job) = answered.job.take() else {
            return answered.into();
        };
        let Some(place) = self.room.place() else {
            return unplaced(serving, &job).await.into();
        };
        Answer {
            left: Some(Left {
                job,
                place,
                from: Some(self.from),
            }),
            ..answered.into()
        }
    }
}

struct Working(Job);

impl<H: Host> Through<H> for Working {
    type Done = Worked;

    async fn through<S: Store, K: Keyring>(
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

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        run: &str,
    ) -> Vec<Swept> {
        poll(
            serving.config,
            serving.connected,
            serving.ports,
            serving.verifier,
            run,
        )
        .await
    }
}

struct Logging;

impl<H: Host> Through<H> for Logging {
    type Done = Result<Option<Publishing>, Stop>;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        _: &str,
    ) -> Result<Option<Publishing>, Stop> {
        publish(serving.config, serving.connected, serving.ports).await
    }
}

struct Queueing<'v>(&'v str, Printed);

impl<H: Host> Through<H> for Queueing<'_> {
    type Done = Result<Board, Stop>;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        _: &str,
    ) -> Result<Board, Stop> {
        queue(
            serving.config,
            serving.connected,
            serving.ports,
            (self.0, self.1),
        )
        .await
    }
}

struct Orphans<'r>(&'r str, Option<&'r RouteId>);

impl<H: Host> Through<H> for Orphans<'_> {
    type Done = Result<Vec<Orphaned>, String>;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        _: &str,
    ) -> Self::Done {
        orphans(serving.ports.store, self.0, self.1).await
    }
}

struct Cleared<'r>(&'r str, (&'r RouteId, &'r str));

impl<H: Host> Through<H> for Cleared<'_> {
    type Done = Result<Orphaned, String>;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        _: &str,
    ) -> Self::Done {
        orphans_cleared(serving.ports.store, self.0, self.1).await
    }
}

struct Reporting;

impl<H: Host> Through<H> for Reporting {
    type Done = Vec<Swept>;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        run: &str,
    ) -> Vec<Swept> {
        report(serving.config, serving.connected, serving.ports, run).await
    }
}

struct Holding<'h, So, T, Ti> {
    sockets: &'h So,
    taker: &'h T,
    timer: &'h Ti,
}

impl<H: Host, So: Sockets, T: Taker, Ti: Timer> Through<H> for Holding<'_, So, T, Ti> {
    type Done = ();

    async fn through<S: Store, K: Keyring>(self, serving: &ServingOn<'_, H, S, K>, _: &str) {
        Sessions::new(serving, self.taker, self.timer)
            .held(self.sockets)
            .await;
    }
}

struct Housekeeping;

impl<H: Host> Through<H> for Housekeeping {
    type Done = Result<Housekept, Stop>;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        run: &str,
    ) -> Result<Housekept, Stop> {
        housekeep_announcing(
            (serving.config, serving.connected, serving.ports),
            serving.keys,
            run,
        )
        .await
    }
}
