//! An event announced to every route that takes it, each step once,
//! finishing what an earlier run left (ADR-0005).

use std::collections::BTreeSet;
use std::convert::Infallible;
use std::num::NonZeroUsize;

use futures_util::future::join_all;
use futures_util::stream::{self, StreamExt};
use herald_config::Config;
use herald_core::{
    Announcement, Entry, Event, Facts, Identity, Key, KindSpecs, Latest, Marker, Marks,
    MentionStyle, Pinning, Plan, Planned, Profile, Reach, Revise, Revision, RouteId, Sent,
    SourceName, State, Step, Templates, Threads, Visibility, plan, revise, revised, schedules,
    steps,
};
use herald_kinds::Signing;
use herald_ports::{
    Announcements, Clock, Http, Inboxes, Ledger, Markers, MarkersError, Notes, Pins, Schedules,
    Secret, Signer, Store, Subscriptions, Version, Written, marker,
};
use jiff::{SignedDuration, Timestamp};

use crate::acting::acting;
use crate::budget::Budget;
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
    /// What the run may still spend before its host stops it.
    pub budget: Budget<'a>,
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
    /// It edits an announcement the route never made; nothing was sent.
    Unedited,
    /// It announces each project as an actor of its own, and the event's
    /// project names none, for this reason; nothing was sent.
    Skipped(String),
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
    /// The host's limit on the run's time or outbound requests was near,
    /// so no further send was started.
    Limit(String),
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
    /// the post or its edit and the route serves what it publishes.
    async fn noted(
        &self,
        step: &Step,
        announcement: &Announcement,
        progress: &Progress,
    ) -> Result<(), Stop> {
        if matches!(step, Step::Post | Step::Edit { part: 0, .. })
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
        (batch, key): (&[String], Key),
        announcement: &Announcement,
        progress: &mut Progress,
    ) -> Result<(), Unlanded> {
        let now = self.ports.clock.now();
        let steps: Vec<Step> = batch.iter().cloned().map(Step::Deliver).collect();
        let unlanded = |reason| (steps.clone(), reason);
        self.write(progress).await.map_err(unlanded)?;
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

    /// Delivers the announcement, as `step` says, to the inbox of each
    /// follower of `actor` it has not landed in, as many at once as its
    /// destination's `deliveries` says, and lands `step` once every one has:
    /// the deliveries to every follower, or the updates of a revision to
    /// each the deliveries reached, each kept under a key of the revision.
    async fn deliveries(
        &self,
        (actor, step): (&str, &Step),
        announcement: &Announcement,
        progress: &mut Progress,
    ) -> Result<(), Unlanded> {
        let whole = |reason| (vec![step.clone()], reason);
        let created = progress.entry.key;
        let key = match step {
            Step::Updates(revision) => created.revised(revision),
            _ => created,
        };
        let mut pending = Vec::new();
        for inbox in self.recipients(actor).await.map_err(whole)? {
            let inboxes = self.ports.store.inboxes();
            let reached = key == created
                || landed(
                    inboxes
                        .delivery(&created, &inbox)
                        .await
                        .map_err(|error| whole(stored(error)))?
                        .as_ref(),
                );
            let state = inboxes
                .delivery(&key, &inbox)
                .await
                .map_err(|error| whole(stored(error)))?;
            if reached && !landed(state.as_ref()) {
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
            let delivered = match self
                .ports
                .budget
                .allows(self.ports.clock.now(), batch.len())
            {
                Ok(()) => self.delivered((batch, key), announcement, progress).await,
                Err(reason) => Err((batch.iter().cloned().map(Step::Deliver).collect(), reason)),
            };
            if let Err((mut failed, reason)) = delivered {
                failed.extend(batches.flatten().cloned().map(Step::Deliver));
                return Err((failed, reason));
            }
        }
        progress.entry = progress
            .entry
            .clone()
            .landed(step.clone(), Facts::default());
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
        if let (Step::Deliveries | Step::Updates(_), Some(actor)) =
            (&step.step, self.route.reached.follows())
        {
            return self
                .deliveries((actor, &step.step), announcement, progress)
                .await;
        }
        self.ports
            .budget
            .allows(self.ports.clock.now(), 1)
            .map_err(|reason| (vec![step.step.clone()], reason))?;
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
            herald_core::itself::VERSION,
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
        let route = &self.route.config.route.id;
        let key = Key::derive(route, event.identity());
        if let Ok(outcome) = self.announced(key, event, templates, Some(Reposted)).await {
            return outcome;
        }
        let reposted = Identity::new(format!("{}{REPOSTED}", event.identity().as_str()));
        let key = Key::derive(route, &reposted);
        let Ok(outcome) = self
            .announced::<Infallible>(key, event, templates, None)
            .await;
        outcome
    }

    /// What the route did with `event` as the announcement `key`; or
    /// `repost`, where it gives one and the route posts a late Release
    /// anew under a key of its own.
    async fn announced<R>(
        &self,
        key: Key,
        event: &Event,
        templates: &Templates,
        repost: Option<R>,
    ) -> Result<Outcome, R> {
        let (mut announcement, wanted, (revision, parts)) =
            match self.prepared(&key, event, templates).await {
                Ok(prepared) => prepared,
                Err(reason) => return Ok(stopped(reason)),
            };
        let (planned, mut progress) = match self
            .claimed(key, &wanted, (&revision, parts, event.edited()), repost)
            .await
        {
            Ok(claimed) => claimed,
            Err(Claim::Reposted(repost)) => return Err(repost),
            Err(Claim::Done(outcome)) => return Ok(outcome),
        };
        let stop = self.taken(&planned, &mut announcement, &mut progress).await;
        Ok(self.released(stop, progress).await)
    }

    /// The announcement `key` of `event`, rendered for the route, the steps
    /// the route wants it sent in, and what it shows in how many parts.
    async fn prepared(
        &self,
        key: &Key,
        event: &Event,
        templates: &Templates,
    ) -> Result<(Announcement, Vec<Step>, (Revision, u32)), Stop> {
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
        let link = announcement.link.as_deref();
        let revision = Revision::of(
            event.signal(),
            std::iter::once(announcement.title.plain())
                .chain(announcement.parts.iter().map(|part| part.body.as_str()))
                .chain(link),
        );
        Ok((announcement, wanted, (revision, parts)))
    }

    /// The steps the announcement `key` still needs, bringing a post that
    /// changed to `revision` where the route edits it, and its ledger entry
    /// claimed for this run; or else what the route did instead: an earlier
    /// run announced it, another holds it, it could not be claimed, or it
    /// is posted anew as `repost`.
    async fn claimed<R>(
        &self,
        key: Key,
        wanted: &[Step],
        (revision, parts, edited): (&Revision, u32, bool),
        repost: Option<R>,
    ) -> Result<(Vec<Planned>, Progress), Claim<R>> {
        let now = self.ports.clock.now();
        let record = self
            .ports
            .store
            .ledger()
            .entry(&key)
            .await
            .map_err(|error| Claim::Done(stopped(stored(error))))?;
        let entry = record.as_ref().map(|record| &record.value);
        if edited && entry.is_none_or(|entry| entry.message(&Step::Post).is_none()) {
            return Err(Claim::Done(Outcome::Unedited));
        }
        let edits = self.route.reached.profile().features.edits;
        let delivery = &self.route.config.route.delivery;
        let revising = entry.map(|entry| (entry, revise(entry, revision, delivery, edits)));
        let (wanted, shown) = match (revising, repost) {
            (Some((_, Revise::Post)), Some(repost)) => return Err(Claim::Reposted(repost)),
            (Some((entry, Revise::Edit)), _) => (revised(entry, revision, parts), revision),
            (Some((entry, _)), _) => match &entry.revision {
                Some(shown) if shown.digest != revision.digest => (wanted.to_vec(), shown),
                _ => (resumed(entry).unwrap_or_else(|| wanted.to_vec()), revision),
            },
            (None, _) => (wanted.to_vec(), revision),
        };
        let planned = match plan(entry, &wanted, self.holder, now) {
            Plan::Complete => return Err(Claim::Done(Outcome::AlreadyAnnounced)),
            Plan::Held { by, until } => return Err(Claim::Done(Outcome::Held { by, until })),
            Plan::Send(planned) => planned,
        };
        let expected = record.as_ref().map(|record| record.version);
        let shown = shown.clone();
        let claimed = record
            .map_or_else(|| Entry::new(key), |record| record.value)
            .claimed_by(
                self.holder,
                now,
                now.checked_add(self.lease).unwrap_or(Timestamp::MAX),
            )
            .wanting(&wanted)
            .revising(shown);
        let version = match self
            .ports
            .store
            .ledger()
            .put_entry(&claimed, expected)
            .await
        {
            Ok(Written::Written(version)) => version,
            Ok(Written::Conflict) => {
                return Err(Claim::Done(stopped(Stop::Store(
                    "another run claimed the announcement first".to_owned(),
                ))));
            }
            Err(error) => return Err(Claim::Done(stopped(stored(error)))),
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

/// What a route's identity is suffixed with, keying a late Release it
/// posts anew apart from its tag's post.
const REPOSTED: &str = "#release";

/// That a route posts its announcement anew under a key of its own.
struct Reposted;

/// Why a route claimed no steps of an announcement: it did something else,
/// or posts it anew, as `R` says.
enum Claim<R> {
    Done(Outcome),
    Reposted(R),
}

/// Whether a delivery stands `state`: landed.
fn landed(state: Option<&State>) -> bool {
    matches!(state, Some(State::Landed { .. }))
}

/// The steps `entry` wants where a revision it brings its post to is under
/// way.
fn resumed(entry: &Entry) -> Option<Vec<Step>> {
    entry
        .wanted
        .clone()
        .filter(|wanted| wanted.iter().any(|step| matches!(step, Step::Edit { .. })))
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
        Outcome::Held { .. }
        | Outcome::Stopped { .. }
        | Outcome::Unedited
        | Outcome::Skipped(_) => return outcome,
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
    announce_within(
        config,
        connected,
        ports,
        source,
        event,
        Within::default(),
        run,
    )
    .await
}

/// The routes an event may reach beside what each route takes: those an
/// app may announce to, and those an owner's member names.
#[derive(Debug, Clone, Copy, Default)]
pub struct Within<'a> {
    /// The routes it may reach, where only some.
    pub routes: Option<&'a [RouteId]>,
    /// The routes the owner's member it is of names, where it is one's.
    pub reach: Option<&'a Reach>,
}

impl<'a> Within<'a> {
    /// Within `routes` alone.
    #[must_use]
    pub const fn routes(routes: &'a [RouteId]) -> Self {
        Self {
            routes: Some(routes),
            reach: None,
        }
    }
}

/// [`announce`], to the routes `within` names alone where it names any: the
/// routes an app may announce to (ADR-0007), or those an owner's member
/// names beside or in place of its source's; and to the `private` routes
/// alone where the event is not public, or is an owner's member's and says
/// nothing of who may see it, and `[doctor] private_to_public` refuses
/// (ADR-0029). As many routes as `[run] routes` says are announced
/// to at once, one at a time where the store writes one record at a time,
/// and what the project last announced is kept for each route in the
/// file's order (ADR-0025).
pub async fn announce_within<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    source: &SourceName,
    event: &Event,
    within: Within<'_>,
    run: &str,
) -> Vec<Announced> {
    let event = &event.clone().through(source);
    let visibility = match (event.bounded_by(), within.reach) {
        (Some(visibility), _) => Some(visibility),
        (None, Some(_)) => Some(Visibility::Private),
        (None, None) => None,
    };
    let bound = visibility.and_then(|visibility| crate::poll::bounded(config, visibility));
    let taking: Vec<&(PreparedRoute, H)> = connected
        .routes
        .iter()
        .filter(|(route, _)| {
            let route = &route.config.route;
            within
                .routes
                .is_none_or(|routes| routes.contains(&route.id))
                && bound
                    .as_ref()
                    .is_none_or(|routes| routes.contains(&route.id))
                && within.reach.map_or_else(
                    || route.filter.takes(source, event),
                    |reach| reach.takes(route, source, event),
                )
        })
        .collect();
    let now = ports.clock.now();
    let outcomes: Vec<Outcome> = stream::iter(taking.iter().map(|(route, http)| async move {
        let route = match acting((connected, ports.store, now), route, (source, event)).await {
            Ok(route) => route,
            Err(outcome) => return outcome,
        };
        Run {
            ports,
            route: &route,
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
