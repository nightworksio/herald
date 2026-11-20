//! Housekeeping: each thread's chores done as they fall due.

use std::collections::BTreeMap;

use herald_config::Config;
use herald_core::{DestinationName, Retention, Schedule, Sent, due};
use herald_ports::{
    Clock, Decisions, Deliveries, Http, Inboxes, Keyring, Ledger, Notes, Published, Pulls,
    Schedules, Signer, Store,
};
use jiff::{SignedDuration, Timestamp};

use crate::announce::{Ports, Stop};
use crate::prepare::Connected;
use crate::verification::Keys;

/// What housekeeping did with one schedule that fell due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HousekeptSchedule {
    /// The schedule.
    pub schedule: Schedule,
    /// What became of it.
    pub outcome: Chored,
}

/// What a round of housekeeping did: each schedule that fell due, the
/// records the store's listings skipped as corrupt since the last round,
/// and each held event that fell due (ADR-0032).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Housekept {
    /// What became of each schedule that fell due.
    pub kept: Vec<HousekeptSchedule>,
    /// The path of each record a listing skipped as corrupt.
    pub skipped: Vec<String>,
    /// What became of each held event that fell due.
    pub released: Vec<crate::Swept>,
}

/// What became of a schedule that fell due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chored {
    /// The chore was done, and the schedule removed.
    Done,
    /// The chore cannot be done, for this reason, and the schedule was
    /// removed.
    Dropped(String),
    /// The chore was not done now, for this reason, and the schedule stays.
    Later(Stop),
}

/// [`housekeep`]'s round, with each held event that fell due announced to
/// the routes that take it under `run` (ADR-0032).
///
/// # Errors
///
/// Where [`housekeep`] stops, or the held events cannot be read.
pub async fn housekeep_announcing<S: Store, C: Clock, G: Signer, H: Http, K: Keyring>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    keys: &Keys<'_, K>,
    run: &str,
) -> Result<Housekept, Stop> {
    let housekept = housekeep(
        connected,
        ports.store,
        ports.clock,
        keys,
        (&config.retention, config.federation.keep_audit),
    )
    .await?;
    let released = crate::announce_held(config, connected, ports, run).await?;
    Ok(Housekept {
        released,
        ..housekept
    })
}

/// Every chore due at the clock's time done through the route that
/// scheduled it, each schedule removed once its chore is done or cannot be
/// (ADR-0005), and the chores of a destination that asked to be asked again
/// later left until the next round, after every key in `keys` read longer ago than its refresh
/// and what the store keeps longer than `retention` says are forgotten: each
/// delivery's id (ADR-0013), each ledger entry with its deliveries to
/// followers' inboxes, and each Note; and each peer's audit of a day
/// longer ago than `keep_audit` (ADR-0054).
///
/// # Errors
///
/// When the keys cannot be forgotten, the store's schedules cannot be read,
/// or what it keeps cannot be expired.
pub async fn housekeep<S: Store, C: Clock, H: Http, K: Keyring>(
    connected: &Connected<H>,
    store: &S,
    clock: &C,
    keys: &Keys<'_, K>,
    (retention, keep_audit): (&Retention, SignedDuration),
) -> Result<Housekept, Stop> {
    keys.sweep(clock.now())
        .await
        .map_err(|error| Stop::Store(error.to_string()))?;
    expire(store, clock.now(), (retention, keep_audit))
        .await
        .map_err(|error| Stop::Store(error.to_string()))?;
    let schedules = store
        .schedules()
        .every_schedule()
        .await
        .map_err(|error| Stop::Store(error.to_string()))?;
    let mut kept = Vec::new();
    let mut waiting: BTreeMap<&DestinationName, Stop> = BTreeMap::new();
    for schedule in due(&schedules, clock.now()) {
        let route = connected
            .routes
            .iter()
            .find(|(route, _)| route.config.route.id == schedule.route);
        if let Some(stop) =
            route.and_then(|(route, _)| waiting.get(&route.config.route.destination))
        {
            kept.push(HousekeptSchedule {
                schedule: schedule.clone(),
                outcome: Chored::Later(stop.clone()),
            });
            continue;
        }
        let outcome = match route {
            None => Chored::Dropped("the route is no longer in the config".to_owned()),
            Some((route, http)) => {
                match route
                    .reached
                    .housekeep(http, schedule.chore, &schedule.thread)
                    .await
                {
                    Ok(Sent::Landed(_)) => Chored::Done,
                    Ok(Sent::Refused(reason)) => Chored::Dropped(reason),
                    Ok(Sent::RetryAfter(after)) => {
                        waiting.insert(&route.config.route.destination, Stop::RetryAfter(after));
                        Chored::Later(Stop::RetryAfter(after))
                    }
                    Err(error) => Chored::Later(Stop::Unreachable(error.to_string())),
                }
            }
        };
        let outcome = match outcome {
            Chored::Later(stop) => Chored::Later(stop),
            settled => match store
                .schedules()
                .put_schedules(&[], std::slice::from_ref(schedule))
                .await
            {
                Ok(()) => settled,
                Err(error) => Chored::Later(Stop::Store(error.to_string())),
            },
        };
        kept.push(HousekeptSchedule {
            schedule: schedule.clone(),
            outcome,
        });
    }
    Ok(Housekept {
        released: Vec::new(),
        kept,
        skipped: store.skipped(),
    })
}

async fn expire<S: Store>(
    store: &S,
    now: Timestamp,
    (retention, keep_audit): (&Retention, SignedDuration),
) -> Result<(), herald_ports::StoreError> {
    let before = |kept: SignedDuration| now.checked_sub(kept).unwrap_or(Timestamp::MIN);
    if let Some(deliveries) = store.deliveries() {
        deliveries
            .expire_deliveries(before(retention.deliveries))
            .await?;
    }
    if let (Some(ledger), Some(kept)) = (store.ledger(), retention.ledger) {
        for key in ledger.expire_entries(before(kept)).await? {
            if let Some(inboxes) = store.inboxes() {
                inboxes.forget_deliveries(&key).await?;
            }
        }
    }
    if let (Some(notes), Some(kept)) = (store.notes(), retention.notes) {
        notes.expire_notes(before(kept)).await?;
    }
    if let Some(published) = store.published() {
        published
            .expire_audits(&herald_core::utc_day(before(keep_audit)))
            .await?;
    }
    if let (Some(decisions), Some(kept)) = (store.decisions(), retention.decisions) {
        decisions.expire_decisions(before(kept)).await?;
    }
    if let (Some(pulls), Some(kept)) = (store.pulls(), retention.pulls) {
        pulls.expire_pulls(before(kept)).await?;
    }
    Ok(())
}
