//! An event announced to every route that takes it, each step once,
//! finishing what an earlier run left (ADR-0005).

use std::convert::Infallible;
use std::num::NonZeroUsize;

use futures_util::stream::{self, StreamExt};
use herald_config::Config;
use herald_core::{
    Announcement, Entry, Event, Facts, HeldEvent, Identity, Key, KindSpecs, Marker, Marks,
    MentionStyle, Pinning, Plan, Planned, Profile, Reach, Revise, Revision, RouteId, Sent, Showing,
    SourceName, State, Step, Subject, Templates, Threads, Visibility, plan, regrouped, revise,
    revised, schedules, steps,
};
use herald_kinds::Signing;
use herald_ports::{
    Clock, Http, Ledger, Markers, MarkersError, Notes, Pins, Schedules, Secret, Signer, Store,
    Subscriptions, Version, Written, marker,
};
use jiff::{SignedDuration, Timestamp};

mod deliveries;
mod feeding;

use crate::acting::acting;
use crate::budget::Budget;
use crate::grouping::Grouped;

mod heard;
mod latest;
mod pushing;
mod replacing;
mod sealing;
mod withdrawing;
mod zoning;

use crate::prepare::{Connected, PreparedRoute};
use heard::Heard;
pub(crate) use latest::kept_latest;
pub(crate) use sealing::sealed;
pub(crate) use withdrawing::{Asked, undeletable};

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
    /// It joined the group of its key, announced as one when due; nothing
    /// was sent.
    Gathered {
        /// The key of its group.
        key: String,
        /// When the group is due.
        due: Timestamp,
    },
    /// Another run holds it; nothing was sent by this one.
    Held {
        /// The run holding it.
        by: String,
        /// When its hold lapses.
        until: Timestamp,
    },
    /// It went to a route a view's advisories section was sent to: sent,
    /// or stopped where `stopped` says, how many steps it took, whether the
    /// post changed and why it stopped untold, so nothing a run says tells
    /// the section's size, when it changed or what the destination answered
    /// (ADR-0067).
    Sealed {
        /// Whether it stopped; the next run sends what remains.
        stopped: bool,
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
    /// An event's envelope could not be proved by the instance's key.
    Proof(String),
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
    /// Who may see the event, as the routing worked it out: its own
    /// visibility, its origin's, private for an owner's member saying
    /// nothing of it, and public otherwise.
    pub(crate) seen: Visibility,
    /// The view the event reports, where it is a report.
    pub(crate) view: Option<Showing<'a>>,
    /// Whether the route is or was sent a view's advisories section: its
    /// living post is then deleted and posted anew where its destination
    /// keeps what an edit replaced (ADR-0067).
    pub(crate) advised: bool,
}

impl<S, C, G, H> Clone for Run<'_, S, C, G, H> {
    fn clone(&self) -> Self {
        Self {
            ports: self.ports,
            route: self.route,
            kinds: self.kinds,
            http: self.http,
            holder: self.holder,
            lease: self.lease,
            seen: self.seen,
            view: self.view,
            advised: self.advised,
        }
    }
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

pub(crate) fn stored(error: impl std::fmt::Display) -> Stop {
    Stop::Store(error.to_string())
}

impl<'a, S: Store, C: Clock, G: Signer, H: Http> Run<'a, S, C, G, H> {
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
        if verify_first && let Some(facts) = self.read_back(step, announcement, progress).await? {
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
        let first = progress.entry.sent_at(step).is_none();
        progress.entry = progress
            .entry
            .clone()
            .attempted(step.clone(), self.ports.clock.now());
        self.write(progress).await?;
        let heard = Heard::new(self.http);
        let sent = self
            .route
            .reached
            .send(
                &heard,
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
            Sent::Refused(reason) => {
                if first && heard.declined(|answer| self.route.reached.declines(answer)) {
                    progress.entry = progress.entry.clone().refused(step);
                }
                Err(Stop::Refused(reason))
            }
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
            .found(&progress.entry.key, &announcement.marks.announcement)
            .await?;
        Ok(found.as_ref().and_then(|found| found.facts(step)).cloned())
    }

    /// What the route's own posts marked `marker` show of the announcement
    /// `key`, where its destination reads them back.
    async fn found(&self, key: &Key, marker: &Marker) -> Result<Option<Entry>, Stop> {
        if !self.route.reached.profile().features.reads_back {
            return Ok(None);
        }
        self.route
            .reached
            .read(self.http)
            .entry(key, marker)
            .await
            .map_err(|error| match error {
                MarkersError::RetryAfter { after } => Stop::RetryAfter(after),
                MarkersError::Unreadable { .. } => Stop::Unread(error.to_string()),
            })
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

    async fn rendered(
        &self,
        subject: Subject<'_>,
        templates: &Templates,
        profile: &Profile,
        marks: Marks,
    ) -> Result<Announcement, Stop> {
        let route = &self.route.config.route;
        let event = subject.event;
        let mut announcement = herald_core::announce(
            subject,
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
            let mut subscribers: Vec<String> = Vec::new();
            for project in projects(subject) {
                let kept = self
                    .ports
                    .store
                    .subscriptions()
                    .subscribers(&route.destination, project)
                    .await
                    .map_err(stored)?;
                for subscription in kept {
                    if !subscribers.contains(&subscription.subscriber) {
                        subscribers.push(subscription.subscriber);
                    }
                }
            }
            let mut messages = room.spread(subscribers).into_iter();
            announcement.mentioned = messages.next().unwrap_or_default();
            announcement.mentioned_after = messages.collect();
        }
        Ok(announcement)
    }

    pub(crate) async fn announce(&self, event: &Event, templates: &Templates) -> Outcome {
        if let Some(fed) = self.route.reached.feeds() {
            return self.fed(fed, event).await;
        }
        if let Some(pushed) = self.route.reached.pushes() {
            return self.pushed(pushed, event).await;
        }
        let route = &self.route.config.route.id;
        let key = Key::derive(route, event.identity());
        if let Ok(outcome) = self
            .announced(key, self.subject(event), templates, Some(Reposted))
            .await
        {
            return outcome;
        }
        self.announce_anew(event, templates).await
    }

    /// What the route did with `event`, a Release posted anew under a key
    /// of its own apart from its tag's post.
    pub(crate) async fn announce_anew(&self, event: &Event, templates: &Templates) -> Outcome {
        let key = Key::derive(&self.route.config.route.id, &reposted(event.identity()));
        let Ok(outcome) = self
            .announced::<Infallible>(key, self.subject(event), templates, None)
            .await;
        outcome
    }

    /// `event` as the route announces it, with the view it reports.
    fn subject<'e>(&self, event: &'e Event) -> Subject<'e>
    where
        'a: 'e,
    {
        Subject {
            view: self.view,
            ..event.into()
        }
    }

    /// What the route did with `subject`, a group, announced under its key:
    /// posted, or its post edited to what the group now renders where the
    /// destination edits.
    pub(crate) async fn announce_group(
        &self,
        subject: Subject<'_>,
        templates: &Templates,
    ) -> Outcome {
        let key = Key::derive(&self.route.config.route.id, &subject.identity());
        let Ok(outcome) = self
            .announced::<Infallible>(key, subject, templates, None)
            .await;
        outcome
    }

    /// What the route did with `event` as the announcement `key`; or
    /// `repost`, where it gives one and the route posts a late Release
    /// anew under a key of its own.
    async fn announced<R>(
        &self,
        key: Key,
        subject: Subject<'_>,
        templates: &Templates,
        repost: Option<R>,
    ) -> Result<Outcome, R> {
        let (mut announcement, wanted, (revision, parts)) =
            match self.prepared(&key, subject, templates).await {
                Ok(prepared) => prepared,
                Err(reason) => return Ok(stopped(reason)),
            };
        let marker = announcement.marks.announcement.clone();
        let (planned, mut progress) = match self
            .claimed(key, &wanted, (&revision, parts, subject), (repost, &marker))
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
        subject: Subject<'_>,
        templates: &Templates,
    ) -> Result<(Announcement, Vec<Step>, (Revision, u32)), Stop> {
        let marks = self.marks(key).await?;
        let profile = Profile {
            symbols: self.route.config.profile.symbols,
            ..self.route.reached.profile()
        };
        let announcement = self.rendered(subject, templates, &profile, marks).await?;
        let parts = u32::try_from(announcement.messages()).unwrap_or(u32::MAX);
        let follows = self.route.reached.follows().is_some();
        let delivery = &self.route.config.route.delivery;
        let wanted = steps(delivery, &profile.features, parts, follows);
        let link = announcement.link.as_deref();
        let revision = Revision::of(
            subject.event.signal(),
            std::iter::once(announcement.title.plain())
                .chain(announcement.parts.iter().map(|part| part.body.as_str()))
                .chain(link),
        );
        Ok((announcement, wanted, (revision, parts)))
    }

    /// The steps the announcement `key` still needs, bringing a post that
    /// changed to `revision` where the route edits it, and its ledger entry
    /// claimed for this run; or else what the route did instead: an earlier
    /// run announced it, alone or in a group, another holds it, it could
    /// not be claimed, or it is posted anew as `repost`.
    async fn claimed<R>(
        &self,
        key: Key,
        wanted: &[Step],
        (revision, parts, subject): (&Revision, u32, Subject<'_>),
        (repost, marker): (Option<R>, &Marker),
    ) -> Result<(Vec<Planned>, Progress), Claim<R>> {
        let edited = subject.group.is_none() && subject.event.edited();
        let now = self.ports.clock.now();
        let record = self
            .ports
            .store
            .ledger()
            .entry(&key)
            .await
            .map_err(|error| Claim::Done(stopped(stored(error))))?;
        let entry = record.as_ref().map(|record| &record.value);
        if entry.is_some_and(|entry| entry.group.is_some()) {
            return Err(Claim::Done(Outcome::AlreadyAnnounced));
        }
        if edited && entry.is_none_or(|entry| entry.message(&Step::Post).is_none()) {
            return Err(Claim::Done(Outcome::Unedited));
        }
        let edits = self.route.reached.profile().features.edits;
        let delivery = &self.route.config.route.delivery;
        let replaces = self.advised && delivery.living && self.route.reached.keeps_edits();
        if replaces
            && entry.is_some_and(|entry| {
                entry.replacing.is_some()
                    || entry
                        .revision
                        .as_ref()
                        .is_some_and(|shown| shown.digest != revision.digest)
            })
            && let Some(record) = record.clone()
        {
            return self
                .replacing(record, (wanted, revision), (subject.event.source(), marker))
                .await;
        }
        let revising = entry.map(|entry| {
            let revised = if subject.group.is_some() || delivery.living {
                regrouped(entry, revision, edits)
            } else {
                revise(entry, revision, delivery, edits)
            };
            (entry, revised)
        });
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
            .through(subject.event.source())
            .wanting(&wanted)
            .revising(shown);
        let progress = self
            .progressed(claimed, expected)
            .await
            .map_err(|reason| Claim::Done(stopped(reason)))?;
        Ok((planned, progress))
    }

    /// `entry` claimed by this run for its lease and written over the
    /// version `expected`, as the progress of the steps it takes.
    async fn progressed(&self, entry: Entry, expected: Option<Version>) -> Result<Progress, Stop> {
        let now = self.ports.clock.now();
        let claimed = entry.claimed_by(
            self.holder,
            now,
            now.checked_add(self.lease).unwrap_or(Timestamp::MAX),
        );
        match self
            .ports
            .store
            .ledger()
            .put_entry(&claimed, expected)
            .await
            .map_err(stored)?
        {
            Written::Written(version) => Ok(Progress {
                entry: claimed,
                version,
                landed: Vec::new(),
            }),
            Written::Conflict => Err(Stop::Store(
                "another run claimed the announcement first".to_owned(),
            )),
        }
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
        if stop.is_none() {
            progress.entry = progress.entry.clone().settled();
        }
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

/// The identity a late Release of `identity` is posted anew under, apart
/// from its tag's post.
pub(crate) fn reposted(identity: &Identity) -> Identity {
    Identity::new(format!("{}{REPOSTED}", identity.as_str()))
}

/// That a route posts its announcement anew under a key of its own.
struct Reposted;

/// Why a route claimed no steps of an announcement: it did something else,
/// or posts it anew, as `R` says.
enum Claim<R> {
    Done(Outcome),
    Reposted(R),
}

/// The projects whose subscribers `subject` mentions: each of its group's
/// members', or its event's.
fn projects(subject: Subject<'_>) -> Vec<&str> {
    match subject.group {
        Some(group) => group.events.iter().map(|event| event.project()).collect(),
        None => vec![subject.event.project()],
    }
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
pub(crate) fn stopped(reason: Stop) -> Outcome {
    Outcome::Stopped {
        landed: Vec::new(),
        remaining: Vec::new(),
        reason,
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

/// Why an event of a private kind reaches no route that says nothing of
/// whether its place is private.
const UNPLACED: &str = "the event's kind belongs in private places, and the route says nothing of whether its place is private";

/// The routes an event may reach beside what each route takes: those an
/// app may announce to, and those an owner's member names.
#[derive(Debug, Clone, Copy, Default)]
pub struct Within<'a> {
    /// The routes it may reach, where only some.
    pub routes: Option<&'a [RouteId]>,
    /// The routes the owner's member it is of names, where it is one's.
    pub reach: Option<&'a Reach>,
    /// The view it reports, where it is a report (ADR-0055).
    pub view: Option<Showing<'a>>,
    /// The routes that are or were sent its view's advisories section: a
    /// living post of one is deleted and posted anew where its destination
    /// keeps what an edit replaced, and what it did is sealed (ADR-0067).
    pub advised: &'a [RouteId],
}

impl<'a> Within<'a> {
    /// Within `routes` alone.
    #[must_use]
    pub const fn routes(routes: &'a [RouteId]) -> Self {
        Self {
            routes: Some(routes),
            reach: None,
            view: None,
            advised: &[],
        }
    }
}

/// The routes of `connected` an event of `source` may reach: those
/// `within` names and `bound` lets through, where either names any, that
/// name `source` or the owner's member `within` names reaches.
fn reachable<'c, H>(
    connected: &'c Connected<H>,
    (within, bound): (Within<'_>, Option<&[RouteId]>),
    source: &SourceName,
) -> Vec<&'c (PreparedRoute, H)> {
    connected
        .routes
        .iter()
        .filter(|(route, _)| {
            let route = &route.config.route;
            let sourced = route.filter.sources.contains(source);
            within
                .routes
                .is_none_or(|routes| routes.contains(&route.id))
                && bound.is_none_or(|routes| routes.contains(&route.id))
                && within
                    .reach
                    .map_or(sourced, |reach| reach.reaches(&route.id, sourced))
        })
        .collect()
}

/// [`announce`], to the routes `within` names alone where it names any: the
/// routes an app may announce to (ADR-0007), or those an owner's member
/// names beside or in place of its source's; and to the `private` routes
/// alone where the event is not public, or is an owner's member's and says
/// nothing of who may see it, and `[doctor] private_to_public` refuses
/// (ADR-0029); an event of a private kind is skipped by a route that says
/// nothing of whether its place is private, where `[doctor] private_kinds`
/// refuses. As many routes as `[run] routes` says are announced
/// to at once, one at a time where the store writes one record at a time,
/// and what the project last announced is kept for each route in the
/// file's order (ADR-0025). A route that groups gathers the event into the
/// group of its key, and keeps what each member's project last announced
/// as it announces the group (ADR-0045).
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
    let seen = visibility.unwrap_or(Visibility::Public);
    let bound = visibility.and_then(|visibility| crate::poll::bounded(config, visibility));
    let reachable = reachable(connected, (within, bound.as_deref()), source);
    let zoned = zoning::zoned((config, connected, ports), (source, event), &reachable).await;
    let taking = zoned.taking(reachable, within, (source, event));
    let now = ports.clock.now();
    let outcomes: Vec<Outcome> =
        stream::iter(taking.iter().map(|&((route, http), event)| async move {
            if !config.places(&route.config, event.kind()) {
                return Outcome::Skipped(UNPLACED.to_owned());
            }
            let route = match acting(
                (connected, ports.store, now),
                route,
                (source, event.project()),
            )
            .await
            {
                Ok(route) => route,
                Err(outcome) => return outcome,
            };
            let running = Run {
                ports,
                route: &route,
                kinds: &config.kinds,
                http,
                holder: run,
                lease: config.announce.lease,
                seen,
                view: within.view,
                advised: within.advised.contains(&route.config.route.id),
            };
            if event.withdrawn() {
                let released = (event.identity(), Some(event.project()));
                let held = HeldEvent {
                    source: source.clone(),
                    member: None,
                    event: event.clone(),
                };
                return Box::pin(running.withdraw(config, released, Some(held), Asked::Forge))
                    .await;
            }
            let event = &match running.reissued(event).await {
                Ok(event) => event,
                Err(reason) => return stopped(reason),
            };
            match &route.config.route.delivery.group {
                Some(grouping) => {
                    Grouped {
                        run: &running,
                        config,
                        grouping,
                    }
                    .announce(source, event)
                    .await
                }
                None => running.announce(event, &config.templates).await,
            }
        }))
        .buffered(at_once(ports.store, config.run.routes).get())
        .collect()
        .await;
    let mut announced = Vec::new();
    for (((route, _), _), outcome) in taking.into_iter().zip(outcomes) {
        let id = &route.config.route.id;
        let outcome = match route.config.route.delivery.group {
            _ if event.withdrawn() => outcome,
            Some(_) => outcome,
            None => {
                kept_latest(
                    ports.store,
                    event,
                    id,
                    outcome,
                    config.announce.latest_attempts,
                )
                .await
            }
        };
        let outcome = if within.advised.contains(id) {
            sealed(outcome)
        } else {
            outcome
        };
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
