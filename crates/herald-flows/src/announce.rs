use std::collections::BTreeSet;

use herald_config::Config;
use herald_core::{
    Announcement, Entry, Event, Facts, Key, Marker, Marks, Pinning, Plan, Planned, Profile,
    RouteId, Sent, SourceName, Step, Templates, Threads, plan, schedules, steps,
};
use herald_ports::{Clock, Http, Secret, Signer, Store, Version, Written, marker};
use jiff::{SignedDuration, Timestamp};

use crate::prepare::{Connected, PreparedRoute};
use crate::reached::Signing;
use crate::together::together;

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
    together: usize,
}

/// The steps a run took that did not land, and why the first of them did
/// not.
type Unlanded = (Vec<Step>, Stop);

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
                Signing {
                    signer: self.ports.signer,
                    now: self.ports.clock.now(),
                },
                step,
                announcement,
                &route.delivery,
                &progress.entry,
            )
            .await
            .map_err(|error| Stop::Unreachable(error.to_string()))?;
        match sent {
            Sent::Landed(facts) => {
                if *step == Step::Post
                    && let Some(published) = self.route.reached.published(announcement)
                {
                    self.ports
                        .store
                        .put_note(&progress.entry.key, &published)
                        .await
                        .map_err(stored)?;
                }
                self.landed(step, facts, progress).await
            }
            Sent::RetryAfter(after) => Err(Stop::RetryAfter(after)),
            Sent::Refused(reason) => Err(Stop::Refused(reason)),
        }
    }

    /// Takes `batch`, each a delivery to one inbox, all at once, and writes
    /// every delivery that landed to the ledger in one write.
    async fn delivered(
        &self,
        batch: &[Step],
        announcement: &Announcement,
        progress: &mut Progress,
    ) -> Result<(), Unlanded> {
        let now = self.ports.clock.now();
        let unlanded = |reason| (batch.to_vec(), reason);
        for step in batch {
            progress.entry = progress.entry.clone().attempted(step.clone(), now);
        }
        self.write(progress).await.map_err(unlanded)?;
        let delivery = &self.route.config.route.delivery;
        let entry = &progress.entry;
        let sent = together(
            batch
                .iter()
                .map(|step| {
                    let signing = Signing {
                        signer: self.ports.signer,
                        now,
                    };
                    self.route
                        .reached
                        .send(self.http, signing, step, announcement, delivery, entry)
                })
                .collect(),
        )
        .await;
        let mut landed = Vec::new();
        let mut failed = Vec::new();
        let mut first = None;
        for (step, sent) in batch.iter().zip(sent) {
            let reason = match sent {
                Ok(Sent::Landed(facts)) => {
                    progress.entry = progress.entry.clone().landed(step.clone(), facts);
                    landed.push(step.clone());
                    continue;
                }
                Ok(Sent::RetryAfter(after)) => Stop::RetryAfter(after),
                Ok(Sent::Refused(reason)) => Stop::Refused(reason),
                Err(error) => Stop::Unreachable(error.to_string()),
            };
            failed.push(step.clone());
            first.get_or_insert(reason);
        }
        self.write(progress).await.map_err(unlanded)?;
        progress.landed.extend(landed);
        first.map_or(Ok(()), |reason| Err((failed, reason)))
    }

    /// Takes `step`, planned at `index`, or the deliveries from it on,
    /// `together` at once, and answers how many steps it took.
    async fn next(
        &self,
        planned: &[Planned],
        index: usize,
        step: &Planned,
        announcement: &mut Announcement,
        progress: &mut Progress,
    ) -> (usize, Result<(), Unlanded>) {
        let batch: Vec<Step> = planned
            .iter()
            .skip(index)
            .map(|planned| planned.step.clone())
            .take_while(|step| matches!(step, Step::Deliver(_)))
            .take(self.together)
            .collect();
        if !batch.is_empty() {
            return (
                batch.len(),
                self.delivered(&batch, announcement, progress).await,
            );
        }
        let taken = self
            .take(&step.step, step.verify_first, announcement, progress)
            .await
            .map_err(|reason| (vec![step.step.clone()], reason));
        (1, taken)
    }

    /// The inboxes an announcement is delivered to: each distinct inbox of
    /// the followers of the actor the route announces as, in order.
    async fn recipients(&self) -> Result<Vec<String>, Stop> {
        let Some(actor) = self.route.reached.follows() else {
            return Ok(Vec::new());
        };
        let followers = self
            .ports
            .store
            .subscribers(&self.route.config.route.destination, actor)
            .await
            .map_err(stored)?;
        let inboxes: BTreeSet<String> = followers
            .into_iter()
            .filter_map(|follower| follower.inbox)
            .collect();
        Ok(inboxes.into_iter().collect())
    }

    async fn rendered(
        &self,
        event: &Event,
        templates: &Templates,
        profile: &Profile,
        marks: Marks,
    ) -> Result<Announcement, Stop> {
        let route = &self.route.config.route;
        let mut announcement = herald_core::announce(
            event,
            route,
            templates,
            profile,
            env!("CARGO_PKG_VERSION"),
            marks,
        )
        .map_err(|error| Stop::Template(error.to_string()))?;
        if route.delivery.mentions.subscribers {
            announcement.mentioned = self
                .ports
                .store
                .subscribers(&route.destination, event.project())
                .await
                .map_err(stored)?
                .into_iter()
                .map(|subscription| subscription.subscriber)
                .collect();
        }
        Ok(announcement)
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
        let mut announcement = match self.rendered(event, templates, &profile, marks).await {
            Ok(announcement) => announcement,
            Err(reason) => return stopped(reason),
        };
        let parts = u32::try_from(announcement.parts.len()).unwrap_or(u32::MAX);
        let recipients = match self.recipients().await {
            Ok(recipients) => recipients,
            Err(reason) => return stopped(reason),
        };
        let wanted = steps(&route.delivery, &profile.features, parts, &recipients);
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
        let mut index = 0;
        for _ in 0..planned.len() {
            let Some(step) = planned.get(index) else {
                break;
            };
            let (taken, result) = self
                .next(&planned, index, step, &mut announcement, &mut progress)
                .await;
            index += taken;
            if let Err((unlanded, reason)) = result {
                let rest = planned.iter().skip(index).map(|step| step.step.clone());
                stop = Some((
                    unlanded.into_iter().chain(rest).collect::<Vec<Step>>(),
                    reason,
                ));
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
            together: config.server.deliveries,
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
