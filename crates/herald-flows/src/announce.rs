//! An event announced to every route that takes it, each step once,
//! finishing what an earlier run left (ADR-0005).

use std::collections::BTreeSet;
use std::num::NonZeroUsize;

use futures_util::future::join_all;
use futures_util::stream::{self, StreamExt};
use herald_config::Config;
use herald_core::{
    Announcement, Entry, Event, Facts, Key, KindSpecs, Latest, Marker, Marks, MentionStyle,
    Pinning, Plan, Planned, Profile, RouteId, Sent, SourceName, State, Step, Templates, Threads,
    plan, schedules, steps,
};
use herald_kinds::Signing;
use herald_ports::{
    Announcements, Clock, Http, Inboxes, Ledger, Markers, MarkersError, Notes, Pins, Schedules,
    Secret, Signer, Store, Subscriptions, Version, Written, marker,
};
use jiff::{SignedDuration, Timestamp};

use crate::prepare::{Connected, PreparedRoute};

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
    /// The destination's own posts could not be read back to learn whether
    /// a step an earlier run attempted landed, so it is not sent again.
    Unread(String),
    /// The store could not be read or written, or another run wrote first.
    Store(String),
    /// A template could not render the event.
    Template(String),
    /// A marker could not be computed.
    Marker(String),
    /// A polled source could not be read.
    Source(String),
}

pub(crate) struct Run<'a, S, C, G, H> {
    pub(crate) ports: &'a Ports<'a, S, C, G>,
    pub(crate) route: &'a PreparedRoute,
    pub(crate) kinds: &'a KindSpecs,
    pub(crate) http: &'a H,
    pub(crate) holder: &'a str,
    pub(crate) lease: SignedDuration,
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
        let until = self
            .ports
            .clock
            .now()
            .checked_add(self.lease)
            .unwrap_or(Timestamp::MAX);
        progress.entry = progress.entry.clone().renewed(self.holder, until);
        let written = self
            .ports
            .store
            .ledger()
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
                .schedules()
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
                .pins()
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
        if verify_first
            && self.route.reached.profile().features.reads_back
            && let Some(facts) = self.read_back(step, announcement, progress).await?
        {
            return self.landed(step, facts, progress).await;
        }
        if *step == Step::Unpin && route.delivery.pin == Some(Pinning::Store) {
            announcement.replaces = self
                .ports
                .store
                .pins()
                .pinned(&route.id)
                .await
                .map_err(stored)?;
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
                self.noted(step, announcement, progress).await?;
                self.landed(step, facts, progress).await
            }
            Sent::RetryAfter(after) => Err(Stop::RetryAfter(after)),
            Sent::Refused(reason) => Err(Stop::Refused(reason)),
        }
    }

    /// The facts of `step` an earlier run landed, read back from the route's
    /// own posts, where they show it landed.
    async fn read_back(
        &self,
        step: &Step,
        announcement: &Announcement,
        progress: &Progress,
    ) -> Result<Option<Facts>, Stop> {
        let found = self
            .route
            .reached
            .read(self.http)
            .entry(&progress.entry.key, &announcement.marks.announcement)
            .await
            .map_err(|error| match error {
                MarkersError::RetryAfter { after } => Stop::RetryAfter(after),
                MarkersError::Unreadable { .. } => Stop::Unread(error.to_string()),
            })?;
        Ok(found.as_ref().and_then(|found| found.facts(step)).cloned())
    }

    /// Keeps the Note the route's actor published with `step`, where it is
    /// the post and the route serves what it publishes.
    async fn noted(
        &self,
        step: &Step,
        announcement: &Announcement,
        progress: &Progress,
    ) -> Result<(), Stop> {
        if *step == Step::Post
            && let Some((actor, published)) = self.route.reached.published(announcement)
        {
            self.ports
                .store
                .notes()
                .put_note(
                    actor,
                    &progress.entry.key,
                    &published,
                    self.ports.clock.now(),
                )
                .await
                .map_err(stored)?;
        }
        Ok(())
    }

    /// Delivers to each inbox in `batch` all at once, once the run still
    /// holds the announcement, and keeps where each delivery stands as its
    /// own record.
    async fn delivered(
        &self,
        batch: &[String],
        announcement: &Announcement,
        progress: &mut Progress,
    ) -> Result<(), Unlanded> {
        let now = self.ports.clock.now();
        let steps: Vec<Step> = batch.iter().cloned().map(Step::Deliver).collect();
        let unlanded = |reason| (steps.clone(), reason);
        self.write(progress).await.map_err(unlanded)?;
        let key = progress.entry.key;
        for inbox in batch {
            self.ports
                .store
                .inboxes()
                .put_delivery(&key, inbox, &State::Attempted { at: now })
                .await
                .map_err(|error| unlanded(stored(error)))?;
        }
        let delivery = &self.route.config.route.delivery;
        let entry = &progress.entry;
        let sent = join_all(steps.iter().map(|step| {
            let signing = Signing {
                signer: self.ports.signer,
                now,
            };
            self.route
                .reached
                .send(self.http, signing, step, announcement, delivery, entry)
        }))
        .await;
        let mut landed = Vec::new();
        let mut failed = Vec::new();
        let mut first = None;
        for ((inbox, step), sent) in batch.iter().zip(&steps).zip(sent) {
            let reason = match sent {
                Ok(Sent::Landed(facts)) => {
                    self.ports
                        .store
                        .inboxes()
                        .put_delivery(&key, inbox, &State::Landed { facts })
                        .await
                        .map_err(|error| unlanded(stored(error)))?;
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
        progress.landed.extend(landed);
        first.map_or(Ok(()), |reason| Err((failed, reason)))
    }

    /// Delivers the announcement to the inbox of each follower of `actor` it
    /// has not landed in, as many at once as its destination's `deliveries`
    /// says, and lands the deliveries once
    /// every one has.
    async fn deliveries(
        &self,
        actor: &str,
        announcement: &Announcement,
        progress: &mut Progress,
    ) -> Result<(), Unlanded> {
        let whole = |reason| (vec![Step::Deliveries], reason);
        let key = progress.entry.key;
        let mut pending = Vec::new();
        for inbox in self.recipients(actor).await.map_err(whole)? {
            let state = self
                .ports
                .store
                .inboxes()
                .delivery(&key, &inbox)
                .await
                .map_err(|error| whole(stored(error)))?;
            if !matches!(state, Some(State::Landed { .. })) {
                pending.push(inbox);
            }
        }
        let together = self
            .route
            .reached
            .federates()
            .map_or(1, herald_kinds::Federates::deliveries);
        let mut batches = pending.chunks(together.max(1));
        while let Some(batch) = batches.next() {
            if let Err((mut failed, reason)) = self.delivered(batch, announcement, progress).await {
                failed.extend(batches.flatten().cloned().map(Step::Deliver));
                return Err((failed, reason));
            }
        }
        progress.entry = progress
            .entry
            .clone()
            .landed(Step::Deliveries, Facts::default());
        self.write(progress).await.map_err(whole)
    }

    /// Takes `step`, delivering to each follower's inbox where it is the
    /// deliveries.
    async fn next(
        &self,
        step: &Planned,
        announcement: &mut Announcement,
        progress: &mut Progress,
    ) -> Result<(), Unlanded> {
        if let (Step::Deliveries, Some(actor)) = (&step.step, self.route.reached.follows()) {
            return self.deliveries(actor, announcement, progress).await;
        }
        self.take(&step.step, step.verify_first, announcement, progress)
            .await
            .map_err(|reason| (vec![step.step.clone()], reason))
    }

    /// The inboxes an announcement is delivered to: each distinct inbox of
    /// the followers of `actor` that are approved and admitted, in order.
    async fn recipients(&self, actor: &str) -> Result<Vec<String>, Stop> {
        let followers = self
            .ports
            .store
            .subscriptions()
            .subscribers(&self.route.config.route.destination, actor)
            .await
            .map_err(stored)?;
        let inboxes: BTreeSet<String> = followers
            .into_iter()
            .filter(|follower| {
                follower.pending.is_none() && self.route.reached.admits(&follower.subscriber)
            })
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
            self.kinds.links(event.kind()),
        )
        .map_err(|error| Stop::Template(error.to_string()))?;
        if route.delivery.mentions.subscribers
            && let MentionStyle::Users(room) = profile.features.mentions
        {
            let subscribers = self
                .ports
                .store
                .subscriptions()
                .subscribers(&route.destination, event.project())
                .await
                .map_err(stored)?
                .into_iter()
                .map(|subscription| subscription.subscriber)
                .collect();
            let mut messages = room.spread(subscribers).into_iter();
            announcement.mentioned = messages.next().unwrap_or_default();
            announcement.mentioned_after = messages.collect();
        }
        Ok(announcement)
    }

    pub(crate) async fn announce(&self, event: &Event, templates: &Templates) -> Outcome {
        let key = Key::derive(&self.route.config.route.id, event.identity());
        let (mut announcement, wanted) = match self.prepared(&key, event, templates).await {
            Ok(prepared) => prepared,
            Err(reason) => return stopped(reason),
        };
        let (planned, mut progress) = match self.claimed(key, &wanted).await {
            Ok(claimed) => claimed,
            Err(outcome) => return outcome,
        };
        let stop = self.taken(&planned, &mut announcement, &mut progress).await;
        self.released(stop, progress).await
    }

    /// The announcement `key` of `event`, rendered for the route, and the
    /// steps the route wants it sent in.
    async fn prepared(
        &self,
        key: &Key,
        event: &Event,
        templates: &Templates,
    ) -> Result<(Announcement, Vec<Step>), Stop> {
        let marks = self.marks(key).await?;
        let profile = Profile {
            symbols: self.route.config.profile.symbols,
            ..self.route.reached.profile()
        };
        let announcement = self.rendered(event, templates, &profile, marks).await?;
        let parts = u32::try_from(announcement.messages()).unwrap_or(u32::MAX);
        let follows = self.route.reached.follows().is_some();
        let delivery = &self.route.config.route.delivery;
        let wanted = steps(delivery, &profile.features, parts, follows);
        Ok((announcement, wanted))
    }

    /// The steps the announcement `key` still needs, and its ledger entry
    /// claimed for this run; or else what the route did instead: an earlier
    /// run announced it, another holds it, or it could not be claimed.
    async fn claimed(
        &self,
        key: Key,
        wanted: &[Step],
    ) -> Result<(Vec<Planned>, Progress), Outcome> {
        let now = self.ports.clock.now();
        let record = self
            .ports
            .store
            .ledger()
            .entry(&key)
            .await
            .map_err(|error| stopped(stored(error)))?;
        let planned = match plan(
            record.as_ref().map(|record| &record.value),
            wanted,
            self.holder,
            now,
        ) {
            Plan::Complete => return Err(Outcome::AlreadyAnnounced),
            Plan::Held { by, until } => return Err(Outcome::Held { by, until }),
            Plan::Send(planned) => planned,
        };
        let expected = record.as_ref().map(|record| record.version);
        let claimed = record
            .map_or_else(|| Entry::new(key), |record| record.value)
            .claimed_by(
                self.holder,
                now,
                now.checked_add(self.lease).unwrap_or(Timestamp::MAX),
            )
            .wanting(wanted);
        let version = match self
            .ports
            .store
            .ledger()
            .put_entry(&claimed, expected)
            .await
        {
            Ok(Written::Written(version)) => version,
            Ok(Written::Conflict) => {
                return Err(stopped(Stop::Store(
                    "another run claimed the announcement first".to_owned(),
                )));
            }
            Err(error) => return Err(stopped(stored(error))),
        };
        let progress = Progress {
            entry: claimed,
            version,
            landed: Vec::new(),
        };
        Ok((planned, progress))
    }

    /// Takes each of `planned` in turn, until one stops: then the steps that
    /// did not land, every step after it among them, and why.
    async fn taken(
        &self,
        planned: &[Planned],
        announcement: &mut Announcement,
        progress: &mut Progress,
    ) -> Option<Unlanded> {
        for (index, current) in planned.iter().enumerate() {
            if let Err((unlanded, reason)) = self.next(current, announcement, progress).await {
                let rest = planned.iter().skip(index + 1).map(|step| step.step.clone());
                return Some((unlanded.into_iter().chain(rest).collect(), reason));
            }
        }
        None
    }

    /// What the route did, once its announcement is released: stopped where
    /// `stop` names steps that did not land or the release failed, and sent
    /// otherwise.
    async fn released(&self, stop: Option<Unlanded>, mut progress: Progress) -> Outcome {
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

/// A route stopped by `reason` before it landed or planned anything.
fn stopped(reason: Stop) -> Outcome {
    Outcome::Stopped {
        landed: Vec::new(),
        remaining: Vec::new(),
        reason,
    }
}

/// `outcome`, once `event` is kept as what its project last announced on
/// `route`, where it was sent or had been and the store keeps that, or
/// stopped by why it could not be kept (ADR-0016). An announcement already
/// complete keeps it again, so a run that could not keep it is made good by
/// the next.
async fn kept_latest<S: Store>(
    store: &S,
    event: &Event,
    route: &RouteId,
    outcome: Outcome,
    attempts: usize,
) -> Outcome {
    let landed = match &outcome {
        Outcome::Sent { steps } => steps.clone(),
        Outcome::AlreadyAnnounced => Vec::new(),
        Outcome::Held { .. } | Outcome::Stopped { .. } => return outcome,
    };
    let Some(announcements) = store.announcements() else {
        return outcome;
    };
    match keep_latest(announcements, event, route, attempts).await {
        Ok(()) => outcome,
        Err(reason) => Outcome::Stopped {
            landed,
            remaining: Vec::new(),
            reason,
        },
    }
}

async fn keep_latest<A: Announcements>(
    announcements: &A,
    event: &Event,
    route: &RouteId,
    attempts: usize,
) -> Result<(), Stop> {
    for _ in 0..attempts {
        let kept = announcements
            .latest(event.project())
            .await
            .map_err(stored)?;
        let expected = kept.as_ref().map(|kept| kept.version);
        let latest = Latest::after(kept.map(|kept| kept.value), event, route);
        if announcements
            .put_latest(&latest, expected)
            .await
            .map_err(stored)?
            != Written::Conflict
        {
            return Ok(());
        }
    }
    Err(Stop::Store(format!(
        "what {} last announced changed under each of {attempts} writes",
        event.project()
    )))
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
    announce_within(config, connected, ports, source, event, None, run).await
}

/// [`announce`], to the routes `within` names alone where it names any: the
/// routes an app may announce to (ADR-0007). As many routes as `[run]
/// routes` says are announced to at once, one at a time where the store
/// writes one record at a time, and what the project last announced is
/// kept for each route in the file's order (ADR-0025).
pub async fn announce_within<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    source: &SourceName,
    event: &Event,
    within: Option<&[RouteId]>,
    run: &str,
) -> Vec<Announced> {
    let taking: Vec<&(PreparedRoute, H)> = connected
        .routes
        .iter()
        .filter(|(route, _)| {
            within.is_none_or(|within| within.contains(&route.config.route.id))
                && route.config.route.filter.takes(source, event)
        })
        .collect();
    let outcomes: Vec<Outcome> = stream::iter(taking.iter().map(|(route, http)| async move {
        Run {
            ports,
            route,
            kinds: &config.kinds,
            http,
            holder: run,
            lease: config.announce.lease,
        }
        .announce(event, &config.templates)
        .await
    }))
    .buffered(at_once(ports.store, config.run.routes).get())
    .collect()
    .await;
    let mut announced = Vec::new();
    for ((route, _), outcome) in taking.into_iter().zip(outcomes) {
        let id = &route.config.route.id;
        let outcome = kept_latest(
            ports.store,
            event,
            id,
            outcome,
            config.announce.latest_attempts,
        )
        .await;
        announced.push(Announced {
            route: id.clone(),
            outcome,
        });
    }
    announced
}

/// How many of a run's routes or sources go at once: `width`, or one where
/// `store` writes one record at a time, so no write is refused for another
/// made at the same time.
pub(crate) fn at_once<S: Store>(store: &S, width: NonZeroUsize) -> NonZeroUsize {
    if store.writes_one_at_a_time() {
        NonZeroUsize::MIN
    } else {
        width
    }
}
