use herald_config::Config;
use herald_core::{
    Announcement, Entry, Event, Facts, Key, Marker, Marks, Pinning, Plan, RouteId, Sent,
    SourceName, Step, Templates, Threads, plan, schedules, steps,
};
use herald_ports::{Clock, Http, Secret, Signer, Store, Version, Written, marker};
use jiff::{SignedDuration, Timestamp};

use crate::prepare::{Connected, PreparedRoute};

/// How long a run holds an announcement before another may finish it.
const LEASE: SignedDuration = SignedDuration::from_mins(10);

/// The ports an announcement goes through, beside each route's `Http`.
#[derive(Debug)]
pub struct Ports<'a, S, C, G> {
    /// Where the ledger and the rest are kept.
    pub store: &'a S,
    /// The time.
    pub clock: &'a C,
    /// What computes each post's markers.
    pub signer: &'a G,
}

/// What one route did with an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announced {
    /// The route.
    pub route: RouteId,
    /// What it did.
    pub outcome: Outcome,
}

/// What a route did with an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Every step it asked for has landed, these by this run.
    Sent {
        /// The steps this run landed.
        steps: Vec<Step>,
    },
    /// An earlier run announced it; nothing was sent.
    AlreadyAnnounced,
    /// Another run holds it; nothing was sent by this one.
    Held {
        /// The run holding it.
        by: String,
        /// When its hold lapses.
        until: Timestamp,
    },
    /// The run stopped part-way; the next run sends what remains.
    Stopped {
        /// The steps this run landed.
        landed: Vec<Step>,
        /// The steps still to send.
        remaining: Vec<Step>,
        /// Why it stopped.
        reason: Stop,
    },
}

/// Why a route stopped part-way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// The destination asked to be asked again after this long.
    RetryAfter(SignedDuration),
    /// The destination refused a step, for this reason.
    Refused(String),
    /// The destination could not be reached.
    Unreachable(String),
    /// The store could not be read or written, or another run wrote first.
    Store(String),
    /// A template could not render the event.
    Template(String),
    /// A marker could not be computed.
    Marker(String),
    /// A polled source could not be read.
    Source(String),
}

struct Run<'a, S, C, G, H> {
    ports: &'a Ports<'a, S, C, G>,
    route: &'a PreparedRoute,
    http: &'a H,
    holder: &'a str,
}

/// Where a route's announcement stands as it is sent.
struct Progress {
    entry: Entry,
    version: Version,
    landed: Vec<Step>,
}

async fn mac<G: Signer>(signer: &G, key: &Secret, message: &[u8]) -> Result<Marker, Stop> {
    marker(signer, key, message)
        .await
        .map_err(|error| Stop::Marker(error.to_string()))
}

fn stored(error: impl std::fmt::Display) -> Stop {
    Stop::Store(error.to_string())
}

impl<S: Store, C: Clock, G: Signer, H: Http> Run<'_, S, C, G, H> {
    async fn marks(&self, key: &Key) -> Result<Marks, Stop> {
        let secret = &self.route.marker_key;
        let route = &self.route.config.route.id;
        Ok(Marks {
            route: mac(self.ports.signer, secret, &Marker::route_message(route)).await?,
            announcement: mac(self.ports.signer, secret, &Marker::message(key)).await?,
        })
    }

    async fn write(&self, progress: &mut Progress) -> Result<(), Stop> {
        let written = self
            .ports
            .store
            .put_entry(&progress.entry, Some(progress.version))
            .await
            .map_err(stored)?;
        match written {
            Written::Written(version) => {
                progress.version = version;
                Ok(())
            }
            Written::Conflict => Err(Stop::Store(
                "another run wrote the announcement's ledger entry first".to_owned(),
            )),
        }
    }

    async fn landed(&self, step: &Step, facts: Facts, progress: &mut Progress) -> Result<(), Stop> {
        progress.entry = progress.entry.clone().landed(step.clone(), facts.clone());
        self.write(progress).await?;
        progress.landed.push(step.clone());
        let route = &self.route.config.route;
        let threads = self.route.reached.profile().features.threads;
        let opened = match step {
            Step::OpenThread | Step::Post if threads != Threads::None => facts.thread,
            _ => None,
        };
        if let (Some(thread), Some(wanted)) = (opened, &route.delivery.thread) {
            let due = schedules(
                progress.entry.key,
                &route.id,
                wanted,
                &thread,
                self.ports.clock.now(),
            );
            self.ports
                .store
                .put_schedules(&due, &[])
                .await
                .map_err(stored)?;
        }
        if *step == Step::Unpin
            && route.delivery.pin == Some(Pinning::Store)
            && let Some(post) = progress
                .entry
                .facts(&Step::Post)
                .and_then(|facts| facts.message.clone())
        {
            self.ports
                .store
                .put_pinned(&route.id, &post)
                .await
                .map_err(stored)?;
        }
        Ok(())
    }

    async fn take(
        &self,
        step: &Step,
        verify_first: bool,
        announcement: &mut Announcement,
        progress: &mut Progress,
    ) -> Result<(), Stop> {
        let route = &self.route.config.route;
        if verify_first && self.route.reached.profile().features.reads_back {
            let found = self
                .route
                .reached
                .landed(
                    self.http,
                    &progress.entry.key,
                    &announcement.marks.announcement,
                )
                .await;
            if let Some(facts) = found.as_ref().and_then(|found| found.facts(step)) {
                return self.landed(step, facts.clone(), progress).await;
            }
        }
        if *step == Step::Unpin && route.delivery.pin == Some(Pinning::Store) {
            announcement.replaces = self.ports.store.pinned(&route.id).await.map_err(stored)?;
            if announcement.replaces.is_none() {
                return self.landed(step, Facts::default(), progress).await;
            }
        }
        progress.entry = progress
            .entry
            .clone()
            .attempted(step.clone(), self.ports.clock.now());
        self.write(progress).await?;
        let sent = self
            .route
            .reached
            .send(
                self.http,
                step,
                announcement,
                &route.delivery,
                &progress.entry,
            )
            .await
            .map_err(|error| Stop::Unreachable(error.to_string()))?;
        match sent {
            Sent::Landed(facts) => self.landed(step, facts, progress).await,
            Sent::RetryAfter(after) => Err(Stop::RetryAfter(after)),
            Sent::Refused(reason) => Err(Stop::Refused(reason)),
        }
    }

    async fn announce(&self, event: &Event, templates: &Templates) -> Outcome {
        let route = &self.route.config.route;
        let key = Key::derive(&route.id, event.identity());
        let stopped = |reason| Outcome::Stopped {
            landed: Vec::new(),
            remaining: Vec::new(),
            reason,
        };
        let marks = match self.marks(&key).await {
            Ok(marks) => marks,
            Err(reason) => return stopped(reason),
        };
        let profile = self.route.reached.profile();
        let rendered = herald_core::announce(
            event,
            route,
            templates,
            &profile,
            env!("CARGO_PKG_VERSION"),
            marks,
        );
        let mut announcement = match rendered {
            Ok(announcement) => announcement,
            Err(error) => return stopped(Stop::Template(error.to_string())),
        };
        let parts = u32::try_from(announcement.parts.len()).unwrap_or(u32::MAX);
        let wanted = steps(&route.delivery, &profile.features, parts, &[]);
        let now = self.ports.clock.now();
        let record = match self.ports.store.entry(&key).await {
            Ok(record) => record,
            Err(error) => return stopped(stored(error)),
        };
        let planned = match plan(
            record.as_ref().map(|record| &record.value),
            &wanted,
            self.holder,
            now,
        ) {
            Plan::Complete => return Outcome::AlreadyAnnounced,
            Plan::Held { by, until } => return Outcome::Held { by, until },
            Plan::Send(planned) => planned,
        };
        let expected = record.as_ref().map(|record| record.version);
        let claimed = record
            .map_or_else(|| Entry::new(key), |record| record.value)
            .claimed(
                self.holder,
                now.checked_add(LEASE).unwrap_or(Timestamp::MAX),
            );
        let version = match self.ports.store.put_entry(&claimed, expected).await {
            Ok(Written::Written(version)) => version,
            Ok(Written::Conflict) => {
                return stopped(Stop::Store(
                    "another run claimed the announcement first".to_owned(),
                ));
            }
            Err(error) => return stopped(stored(error)),
        };
        let mut progress = Progress {
            entry: claimed,
            version,
            landed: Vec::new(),
        };
        let mut stop = None;
        for (index, step) in planned.iter().enumerate() {
            if let Err(reason) = self
                .take(
                    &step.step,
                    step.verify_first,
                    &mut announcement,
                    &mut progress,
                )
                .await
            {
                let remaining = planned.iter().skip(index).map(|step| step.step.clone());
                stop = Some((remaining.collect::<Vec<Step>>(), reason));
                break;
            }
        }
        progress.entry = progress.entry.clone().released();
        let released = self.write(&mut progress).await;
        match (stop, released) {
            (Some((remaining, reason)), _) => Outcome::Stopped {
                landed: progress.landed,
                remaining,
                reason,
            },
            (None, Err(reason)) => Outcome::Stopped {
                landed: progress.landed,
                remaining: Vec::new(),
                reason,
            },
            (None, Ok(())) => Outcome::Sent {
                steps: progress.landed,
            },
        }
    }
}

/// `event`, from `source`, sent to every route of `connected` that takes it,
/// each step once (ADR-0005). `run` names this run in the leases it takes.
pub async fn announce<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    source: &SourceName,
    event: &Event,
    run: &str,
) -> Vec<Announced> {
    let mut announced = Vec::new();
    for (route, http) in &connected.routes {
        if !route.config.route.filter.takes(source, event) {
            continue;
        }
        let outcome = Run {
            ports,
            route,
            http,
            holder: run,
        }
        .announce(event, &config.templates)
        .await;
        announced.push(Announced {
            route: route.config.route.id.clone(),
            outcome,
        });
    }
    announced
}
