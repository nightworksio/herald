//! A route's events that share a key gathered into one announcement: held
//! until the group is due, or posted at once and edited as members come
//! (ADR-0045).

use herald_config::Config;
use herald_core::{
    Change, Context, Entry, Event, GROUP_KEY_MOST, Group, GroupLate, GroupMode, GroupUntil,
    Grouping, HeldEvent, Hold, Key, Pending, Revise, RouteId, SourceName, Subject, Templates,
    Visibility, group_identity, group_template, shown,
};
use herald_kinds::Remembers;
use herald_ports::{Clock, Http, Ledger, Signer, Store, Versioned, Waiting, Written};
use jiff::Timestamp;

use crate::announce::{Announced, Outcome, Ports, Run, Stop, kept_latest, stopped, stored};
use crate::poll::{Fresh, Sweep, Swept};
use crate::prepare::Connected;

mod withdrawn;

/// Why a member that came after its group was announced is let go, where
/// its route's `late` skips it.
const LATE: &str = "its group was announced before it came";

/// Why a group was announced to no one: no member it gathered may reach
/// the route under the config now.
const UNADMITTED: &str = "no member it gathered may reach the route now, as [doctor] private_to_public and the route's private say";

/// A route that groups, announcing as `run` within `config`.
pub(crate) struct Grouped<'a, 'r, S, C, G, H> {
    pub(crate) run: &'a Run<'r, S, C, G, H>,
    pub(crate) config: &'a Config,
    pub(crate) grouping: &'a Grouping,
}

/// What a group's record does with a member that comes.
#[derive(Debug)]
enum Taking {
    /// It takes it, in place of the member of its identity where it has
    /// one.
    Took(Pending),
    /// It holds no member the member edits.
    Unedited,
    /// It holds as many members as it takes.
    Full,
}

/// What a route does with a member that came after its group was
/// announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Late {
    /// Lets it go unannounced.
    Skip,
    /// Announces it alone.
    Alone,
    /// Edits the group's post to take it.
    Join,
}

/// The key `event` is grouped by on `route`, rendered by `templates`; none
/// where it renders empty or longer than a key may be, and the event is
/// announced alone.
fn key_of(templates: &Templates, route: &RouteId, event: &Event) -> Result<Option<String>, Stop> {
    let key = templates
        .render_line(
            &group_template(route),
            &Context {
                event,
                route,
                herald: herald_core::itself::VERSION,
                group: None,
            },
        )
        .map_err(|error| Stop::Template(error.to_string()))?;
    Ok((!key.is_empty() && key.len() <= GROUP_KEY_MOST).then_some(key))
}

/// `pending` taking `held`, as many members at most as `most`.
fn took(mut pending: Pending, held: HeldEvent, most: usize) -> Taking {
    let identity = held.event.identity();
    let at = pending
        .events
        .iter()
        .position(|member| member.event.identity() == identity);
    let count = pending.events.len();
    if let Some(member) = at.and_then(|at| pending.events.get_mut(at)) {
        *member = held;
    } else if held.event.edited() {
        return Taking::Unedited;
    } else if count >= most {
        return Taking::Full;
    } else {
        pending.events.push(held);
    }
    Taking::Took(pending)
}

/// Whether a member coming at `now` to the group `pending` keeps, gathered
/// as `mode` says, comes late: once held, after the group closed to be
/// announced; once edited, after its post landed and its window passed.
fn late_at(mode: GroupMode, pending: &Pending, now: Timestamp) -> bool {
    match mode {
        GroupMode::Hold => pending.closed.is_some(),
        GroupMode::Edit => pending.landed && pending.due_at(now),
    }
}

/// What a route whose `late` is this does with a late member, where a
/// member joining its post revises it as `joining` says.
const fn late_as(late: GroupLate, joining: Revise) -> Late {
    match (late, joining) {
        (GroupLate::Skip, _) => Late::Skip,
        (GroupLate::Edit, Revise::Edit) => Late::Join,
        _ => Late::Alone,
    }
}

/// Whether the group `pending` keeps is announced now that `event` joined
/// it at `now`: at once where it edits, closed already or is due, and as
/// soon as its lead comes where it is held until then.
fn due_now(grouping: &Grouping, pending: &Pending, event: &Event, now: Timestamp) -> bool {
    grouping.mode == GroupMode::Edit
        || pending.closed.is_some()
        || pending.due_at(now)
        || (grouping.until == GroupUntil::Lead && grouping.leads(event))
}

/// Whether a round announces the group `pending` keeps at `now`: where its
/// announcement has not landed, once it closed or is due.
pub(crate) fn flushed_at(pending: &Pending, now: Timestamp) -> bool {
    !pending.landed && (pending.closed.is_some() || pending.due_at(now))
}

/// Whether `pending` fits a record of a store that keeps `most` bytes in
/// one, where it bounds them.
fn fits(pending: &Pending, most: Option<usize>) -> bool {
    most.is_none_or(|most| pending.bytes().saturating_add(Remembers::REST) <= most)
}

impl<S: Store, C: Clock, G: Signer, H: Http> Grouped<'_, '_, S, C, G, H> {
    fn route(&self) -> &RouteId {
        &self.run.route.config.route.id
    }

    fn edits(&self) -> bool {
        self.run.route.reached.profile().features.edits
    }

    fn hold(&self, key: &str) -> Hold {
        Hold::Group {
            route: self.route().clone(),
            key: key.to_owned(),
        }
    }

    /// What the route did with `event`, from `source`: gathered into the
    /// group of its key, the group announced where it is due; announced
    /// alone where its key names no group; or, where a group announced it,
    /// that group's post brought to it as the route says.
    pub(crate) async fn announce(&self, source: &SourceName, event: &Event) -> Outcome {
        let bounded = bounded(event, self.run.seen);
        let event = &bounded;
        let key = match key_of(&self.config.templates, self.route(), event) {
            Ok(Some(key)) => key,
            Ok(None) => return self.alone(event).await,
            Err(reason) => return stopped(reason),
        };
        let member = Key::derive(self.route(), event.identity());
        match self.run.ports.store.ledger().entry(&member).await {
            Err(error) => stopped(stored(error)),
            Ok(None) => self.gather(source, &key, event, None).await,
            Ok(Some(kept)) => match (kept.value.group, kept.value.revision) {
                (Some(group), Some(was)) => match Change::of(&was, event) {
                    Some(change) => self.changed(source, &group, change, event).await,
                    None => self.latest(event, Outcome::AlreadyAnnounced).await,
                },
                _ => self.alone(event).await,
            },
        }
    }

    /// `outcome`, once `event` is kept as what its project last announced
    /// on the route.
    async fn latest(&self, event: &Event, outcome: Outcome) -> Outcome {
        kept_latest(
            self.run.ports.store,
            event,
            self.route(),
            outcome,
            self.config.announce.latest_attempts,
        )
        .await
    }

    /// What the route did with `event`, announced alone.
    async fn alone(&self, event: &Event) -> Outcome {
        let outcome = self.run.announce(event, &self.config.templates).await;
        self.latest(event, outcome).await
    }

    /// What the route did with `event`, a member of the group `key` it
    /// announced, which `change` changes.
    async fn changed(
        &self,
        source: &SourceName,
        key: &str,
        change: Change,
        event: &Event,
    ) -> Outcome {
        let delivery = &self.run.route.config.route.delivery;
        match change.revise(delivery, self.edits()) {
            Revise::Edit => self.gather(source, key, event, Some(change)).await,
            revised => self.unkept(revised, event).await,
        }
    }

    /// What the route did with `event` where its group's post is not
    /// brought to it: posted anew where `revised` says, and nothing more
    /// otherwise.
    async fn unkept(&self, revised: Revise, event: &Event) -> Outcome {
        let outcome = match revised {
            Revise::Post => self.run.announce_anew(event, &self.config.templates).await,
            _ => Outcome::AlreadyAnnounced,
        };
        self.latest(event, outcome).await
    }

    /// What the route did with a member that came after the group `key`
    /// was announced, its record gone.
    async fn late(&self, event: &Event) -> Outcome {
        match self.grouping.late {
            GroupLate::Skip => Outcome::Skipped(LATE.to_owned()),
            GroupLate::Post | GroupLate::Edit => self.alone(event).await,
        }
    }

    /// Whether the route announced the group `key` before, or the route
    /// stopped where its ledger cannot be read.
    async fn announced_before(&self, key: &str) -> Result<bool, Outcome> {
        let group = Key::derive(self.route(), &group_identity(key));
        self.run
            .ports
            .store
            .ledger()
            .entry(&group)
            .await
            .map(|entry| entry.is_some())
            .map_err(|error| stopped(stored(error)))
    }

    /// What the route did with `event`, from `source`, joining the group
    /// `key`, or taking its member's place where `change` changes one: the
    /// group's record written with it, and the group announced where it is
    /// due.
    async fn gather(
        &self,
        source: &SourceName,
        key: &str,
        event: &Event,
        change: Option<Change>,
    ) -> Outcome {
        let hold = self.hold(key);
        let waiting = self.run.ports.store.waiting();
        let joining = HeldEvent {
            source: source.clone(),
            member: None,
            event: event.clone(),
        };
        let attempts = self.config.announce.latest_attempts;
        for _ in 0..attempts {
            let kept = match waiting.pending(&hold).await {
                Ok(kept) => kept,
                Err(error) => return stopped(stored(error)),
            };
            let now = self.run.ports.clock.now();
            let (pending, expected) = match self
                .taken((key, event, change), kept, (joining.clone(), now))
                .await
            {
                Ok(taken) => taken,
                Err(outcome) => return outcome,
            };
            if !fits(&pending, self.config.store.most_record()) {
                return self.alone(event).await;
            }
            match waiting.put_pending(&pending, expected).await {
                Err(error) => return stopped(stored(error)),
                Ok(Written::Conflict) => {}
                Ok(Written::Written(version)) => {
                    return self
                        .written(
                            key,
                            event,
                            Versioned {
                                value: pending,
                                version,
                            },
                            now,
                        )
                        .await;
                }
            }
        }
        stopped(Stop::Store(format!(
            "the group {key} changed under each of {attempts} writes"
        )))
    }

    /// The record of the group `key` taking `joining` at `now` in place of
    /// `kept`, with the version it was read at, or what the route did
    /// instead with `event`, which `change` changes where it is a member's.
    async fn taken(
        &self,
        (key, event, change): (&str, &Event, Option<Change>),
        kept: Option<Versioned<Pending>>,
        (joining, now): (HeldEvent, Timestamp),
    ) -> Result<(Pending, Option<herald_ports::Version>), Outcome> {
        let delivery = &self.run.route.config.route.delivery;
        let kept = match (kept, change) {
            (None, Some(change)) => {
                return Err(self.unkept(change.revise(delivery, false), event).await);
            }
            (None, None) => {
                if self.announced_before(key).await? {
                    return Err(self.late(event).await);
                }
                if event.edited() {
                    return Err(Outcome::Unedited);
                }
                return Ok((self.opened(self.hold(key), joining, now), None));
            }
            (Some(kept), _) => kept,
        };
        let late = change.is_none() && late_at(self.grouping.mode, &kept.value, now);
        match late_as(
            self.grouping.late,
            Change::Joined.revise(delivery, self.edits()),
        ) {
            Late::Skip if late => return Err(Outcome::Skipped(LATE.to_owned())),
            Late::Alone if late => return Err(self.alone(event).await),
            _ => {}
        }
        match took(kept.value, joining, self.grouping.most) {
            Taking::Took(pending) => Ok((pending, Some(kept.version))),
            Taking::Unedited => Err(Outcome::Unedited),
            Taking::Full => Err(self.alone(event).await),
        }
    }

    /// What the route did once the record `kept` of the group `key` was
    /// written with `event` at `now`: the group announced where it is due,
    /// and gathered otherwise.
    async fn written(
        &self,
        key: &str,
        event: &Event,
        kept: Versioned<Pending>,
        now: Timestamp,
    ) -> Outcome {
        if due_now(self.grouping, &kept.value, event, now) {
            return self.flush(key, kept, event).await;
        }
        Outcome::Gathered {
            key: key.to_owned(),
            due: kept.value.due,
        }
    }

    /// The record of a group `hold` opens at `now` with `first`, due once
    /// its window passes.
    fn opened(&self, hold: Hold, first: HeldEvent, now: Timestamp) -> Pending {
        Pending {
            hold,
            events: vec![first],
            opened: now,
            due: now
                .checked_add(self.grouping.window)
                .unwrap_or(Timestamp::MAX),
            closed: None,
            landed: false,
        }
    }

    /// What the route did with the group `key` its record `kept` holds,
    /// announced as one, its lead first, or `by`, the event that brought it
    /// due, where it holds none: a held group closed to members first, so a
    /// member coming after is late; once it landed, each member's entry
    /// records it was announced in the group, the record that it landed,
    /// and each member's project what it last announced.
    async fn flush(&self, key: &str, kept: Versioned<Pending>, by: &Event) -> Outcome {
        let kept = match self.closed(key, kept).await {
            Ok(kept) => kept,
            Err(reason) => return stopped(reason),
        };
        let events: Vec<Event> = kept
            .value
            .events
            .iter()
            .filter(|held| admitted(self.config, self.route(), &held.event))
            .map(|held| held.event.clone())
            .collect();
        if events.is_empty() {
            return match self.recorded(key, &events).await {
                Ok(()) => Outcome::Skipped(UNADMITTED.to_owned()),
                Err(reason) => stopped(reason),
            };
        }
        let mut run = self.run.clone();
        run.seen = strictest(&kept.value).max(self.run.seen);
        let ordered = self.grouping.ordered(&events);
        let subject = Subject {
            event: ordered.first().copied().unwrap_or(by),
            group: Some(Group {
                key,
                events: &ordered,
            }),
        };
        let outcome = run.announce_group(subject, &self.config.templates).await;
        if !matches!(outcome, Outcome::Sent { .. } | Outcome::AlreadyAnnounced) {
            return outcome;
        }
        let mut outcome = match self.recorded(key, &events).await {
            Ok(()) => outcome,
            Err(reason) => return landed_but(outcome, reason),
        };
        for event in &events {
            outcome = self.latest(event, outcome).await;
        }
        outcome
    }

    /// The record `kept` of the group `key`, closed to members now where it
    /// is held and still open.
    async fn closed(
        &self,
        key: &str,
        kept: Versioned<Pending>,
    ) -> Result<Versioned<Pending>, Stop> {
        if self.grouping.mode == GroupMode::Edit || kept.value.closed.is_some() {
            return Ok(kept);
        }
        let closed = Pending {
            closed: Some(self.run.ports.clock.now()),
            ..kept.value
        };
        match self
            .run
            .ports
            .store
            .waiting()
            .put_pending(&closed, Some(kept.version))
            .await
            .map_err(stored)?
        {
            Written::Written(version) => Ok(Versioned {
                value: closed,
                version,
            }),
            Written::Conflict => Err(raced(key)),
        }
    }

    /// Each of `events` recorded as announced in the group `key`, and its
    /// record as landed.
    async fn recorded(&self, key: &str, events: &[Event]) -> Result<(), Stop> {
        let now = self.run.ports.clock.now();
        let ledger = self.run.ports.store.ledger();
        for event in events {
            let member = Key::derive(self.route(), event.identity());
            let entry = Entry::grouped(member, key, shown(event), now);
            let expected = ledger
                .entry(&member)
                .await
                .map_err(stored)?
                .map(|kept| kept.version);
            if ledger.put_entry(&entry, expected).await.map_err(stored)? == Written::Conflict {
                return Err(Stop::Store(format!(
                    "another run wrote the entry of {} first",
                    event.identity().as_str()
                )));
            }
        }
        let hold = self.hold(key);
        let waiting = self.run.ports.store.waiting();
        for _ in 0..self.config.announce.latest_attempts {
            let pending = waiting.pending(&hold).await.map_err(stored)?;
            let Some(kept) = pending.filter(|kept| !kept.value.landed) else {
                return Ok(());
            };
            let landed = Pending {
                landed: true,
                ..kept.value
            };
            if let Written::Written(_) = waiting
                .put_pending(&landed, Some(kept.version))
                .await
                .map_err(stored)?
            {
                return Ok(());
            }
        }
        Err(raced(key))
    }
}

/// `event`, bounded by who the routing saw may see it, `seen`, where it
/// names no one itself, so the group's record holds the bound its member
/// was gathered under.
fn bounded(event: &Event, seen: Visibility) -> Event {
    match event.bounded_by() {
        None if seen != Visibility::Public => event.clone().seen_as(seen),
        _ => event.clone(),
    }
}

/// Whether `config` lets `event`, a member gathered under the config of
/// its time, reach `route` now: never where who may see it bounds it to
/// routes that leave this one out (ADR-0029).
fn admitted(config: &Config, route: &RouteId, event: &Event) -> bool {
    event
        .bounded_by()
        .and_then(|visibility| crate::poll::bounded(config, visibility))
        .is_none_or(|routes| routes.contains(route))
}

/// Why a group's record could not be written: another run wrote it first.
fn raced(key: &str) -> Stop {
    Stop::Store(format!("another run wrote the group {key} first"))
}

/// What a round did with the group `key` of `route` its record `kept`
/// holds, under `run`: forgotten where the route no longer groups or the
/// group's ledger entry would be, and announced where it is due.
pub(crate) async fn round<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    run: &str,
    (route, key): (&RouteId, &str),
    kept: Versioned<Pending>,
) -> Option<Swept> {
    let now = ports.clock.now();
    let grouped = connected.routes.iter().find_map(|(prepared, http)| {
        let grouping = prepared.config.route.delivery.group.as_ref();
        (prepared.config.route.id == *route).then_some((prepared, http, grouping?))
    });
    let lead = kept.value.events.first().cloned();
    let (Some((prepared, http, grouping)), Some(lead)) = (grouped, lead) else {
        return forgotten(ports.store, route, &kept.value.hold).await;
    };
    if kept.value.expired(now, config.retention.ledger) {
        return forgotten(ports.store, route, &kept.value.hold).await;
    }
    if !flushed_at(&kept.value, now) {
        return None;
    }
    let holder = format!("{run}/{}", route.as_str());
    let running = Run {
        ports,
        route: prepared,
        kinds: &config.kinds,
        http,
        holder: &holder,
        lease: config.announce.lease,
        seen: strictest(&kept.value),
    };
    let outcome = Grouped {
        run: &running,
        config,
        grouping,
    }
    .flush(key, kept, &lead.event)
    .await;
    Some(Swept {
        source: lead.source,
        member: None,
        outcome: Sweep::Released {
            announced: vec![Fresh {
                event: lead.event,
                announced: vec![Announced {
                    route: route.clone(),
                    outcome,
                }],
                held: None,
            }],
            withheld: None,
        },
    })
}

/// Who may see the strictest of the members `pending` holds, each as its
/// origin bounds it; anyone where none is bounded.
fn strictest(pending: &Pending) -> Visibility {
    pending
        .events
        .iter()
        .filter_map(|held| held.event.bounded_by())
        .max()
        .unwrap_or(Visibility::Public)
}

/// The record `hold` keeps of a group of `route`, forgotten; or why it
/// could not be, said under the route's name.
async fn forgotten<S: Store>(store: &S, route: &RouteId, hold: &Hold) -> Option<Swept> {
    let error = store.waiting().drop_pending(hold).await.err()?;
    Some(Swept {
        source: SourceName::new(route.as_str()),
        member: None,
        outcome: Sweep::Stopped(stored(error)),
    })
}

/// `outcome`, whose steps landed, stopped by `reason`.
fn landed_but(outcome: Outcome, reason: Stop) -> Outcome {
    let landed = match outcome {
        Outcome::Sent { steps } => steps,
        _ => Vec::new(),
    };
    Outcome::Stopped {
        landed,
        remaining: Vec::new(),
        reason,
    }
}

#[cfg(test)]
mod tests;
