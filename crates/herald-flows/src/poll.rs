//! Each source asked for what it has not seen, and each event it finds
//! announced (ADR-0003).

use futures_util::stream::{self, StreamExt};
use herald_config::Config;
use herald_core::{Cursor, Event, SourceName, fresh};
use herald_kinds::{Poll, Polling, Unpolled};
use herald_ports::{Clock, Cursors, Http, Signer, Store, StoreError, Written};
use jiff::Timestamp;

use crate::announce::{Announced, Outcome, Ports, Stop, announce, at_once};
use crate::prepare::{Connected, PreparedSource};

/// What polling one source, or one member of an owner source, did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Swept {
    /// The source.
    pub source: SourceName,
    /// The member of an owner source polled, by its path, where it is one.
    pub member: Option<String>,
    /// What it did.
    pub outcome: Sweep,
}

/// What polling a source did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sweep {
    /// Nothing changed since the last poll.
    Unchanged,
    /// The source answered.
    Polled {
        /// Whether this was the source's first poll, which announces only
        /// its newest `backfill` (ADR-0003).
        first: bool,
        /// How many events the source answered with.
        found: usize,
        /// Each event announced, the oldest first.
        announced: Vec<Fresh>,
        /// What became of the source's cursor.
        cursor: Remembered,
    },
    /// An owner source was listed, or its listing was unchanged (ADR-0029).
    Listed {
        /// How many members it polls.
        members: usize,
        /// The path of each member that joined.
        joined: Vec<String>,
        /// The path of each member that left.
        left: Vec<String>,
    },
    /// The source could not be polled, for this reason; the next poll asks
    /// again.
    Stopped(Stop),
}

/// An event a poll announced, and what every route did with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fresh {
    /// The event.
    pub event: Event,
    /// What every route that takes it did with it.
    pub announced: Vec<Announced>,
}

/// What became of a source's cursor after a poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remembered {
    /// It remembers every event found.
    Advanced,
    /// It stays as it was, as a route stopped, so the next poll offers the
    /// same events again.
    Kept,
    /// It could not be written, for this reason, so the next poll offers the
    /// same events again.
    Failed(Stop),
}

pub(crate) fn stored(error: impl std::fmt::Display) -> Stop {
    Stop::Store(error.to_string())
}

/// What one poll found and announced, and the cursor to remember where
/// every route has its events.
pub(crate) struct Found {
    pub(crate) count: usize,
    pub(crate) announced: Vec<Fresh>,
    pub(crate) advanced: Option<Cursor>,
}

/// Where a poll that found nothing to read stops.
pub(crate) fn halted(unpolled: Unpolled) -> Stop {
    match unpolled {
        Unpolled::Unreachable(reason) => Stop::Unreachable(reason),
        Unpolled::Unreadable(reason) => Stop::Source(reason),
    }
}

/// The events of `found` to announce: those `cursor` has not seen, or with
/// no cursor every one from `since` on where it is given, and otherwise
/// the newest `backfill`.
fn unseen(
    cursor: Option<&Cursor>,
    found: &[Event],
    since: Option<Timestamp>,
    backfill: usize,
) -> Vec<Event> {
    match (cursor, since) {
        (None, Some(since)) => found
            .iter()
            .filter(|event| event.occurred() >= since)
            .cloned()
            .collect(),
        _ => fresh(cursor, found.to_vec(), backfill),
    }
}

/// What `poll` answers through `http` since `cursor`, each event it has
/// not seen announced as `source`'s under `holder`, or nothing where it
/// says nothing changed. Where `since` is given and there is no cursor,
/// every event from then on is fresh; otherwise only the newest `backfill`.
pub(crate) async fn found<S: Store, C: Clock, G: Signer, H: Http>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    source: &SourceName,
    (poll, http): (&dyn Poll, &H),
    (cursor, since, backfill): (Option<&Cursor>, Option<Timestamp>, usize),
    holder: &str,
) -> Result<Option<Found>, Stop> {
    let Some(releases) = poll.polled(http, cursor).await.map_err(halted)? else {
        return Ok(None);
    };
    let fresh_events = unseen(cursor, &releases.events, since, backfill);
    let mut announced = Vec::new();
    let mut settled = true;
    for event in fresh_events.into_iter().rev() {
        let outcomes = announce(config, connected, ports, source, &event, holder).await;
        settled &= !outcomes
            .iter()
            .any(|outcome| matches!(outcome.outcome, Outcome::Stopped { .. }));
        announced.push(Fresh {
            event,
            announced: outcomes,
        });
    }
    let advanced = settled.then(|| {
        cursor.cloned().unwrap_or_default().advanced(
            &releases.events,
            releases.etag,
            releases.last_modified,
            poll.remembered(),
        )
    });
    Ok(Some(Found {
        count: releases.events.len(),
        announced,
        advanced,
    }))
}

/// What became of a write of a cursor.
pub(crate) fn remembered(written: Result<Written, StoreError>, what: &str) -> Remembered {
    match written {
        Ok(Written::Written(_)) => Remembered::Advanced,
        Ok(Written::Conflict) => {
            Remembered::Failed(Stop::Store(format!("another run moved {what} first")))
        }
        Err(error) => Remembered::Failed(stored(error)),
    }
}

async fn sweep<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    source: &PreparedSource,
    poll: &dyn Poll,
    http: &H,
    run: &str,
) -> Result<Sweep, Stop> {
    let ports = flow.2;
    ports.budget.allows(ports.clock.now(), 1)?;
    let record = ports
        .store
        .cursors()
        .cursor(&source.name)
        .await
        .map_err(stored)?;
    let cursor = record.as_ref().map(|record| &record.value);
    let holder = format!("{run}/{}", source.name.as_str());
    let Some(found) = found(
        flow,
        &source.name,
        (poll, http),
        (cursor, None, source.backfill),
        &holder,
    )
    .await?
    else {
        return Ok(Sweep::Unchanged);
    };
    let cursor = match &found.advanced {
        Some(advanced) => {
            let expected = record.as_ref().map(|record| record.version);
            let written = ports
                .store
                .cursors()
                .put_cursor(&source.name, advanced, expected)
                .await;
            remembered(written, "the source's cursor")
        }
        None => Remembered::Kept,
    };
    Ok(Sweep::Polled {
        first: record.is_none(),
        found: found.count,
        announced: found.announced,
        cursor,
    })
}

/// Every source of `connected` polled, and each event it has not seen
/// announced to every route that takes it (ADR-0003). A source's cursor
/// remembers what it found only once every route has its events, so a
/// route that stopped is offered them again by the next poll. As many
/// sources as `[run] sources` says are polled at once, one at a time where
/// the store writes one record at a time (ADR-0025). Each source takes its
/// leases under `run` and its own name, so two sources finding one event
/// never both send it.
pub async fn poll<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    run: &str,
) -> Vec<Swept> {
    let flow = (config, connected, ports);
    let swept: Vec<Vec<Swept>> =
        stream::iter(connected.sources.iter().map(|(source, http)| async move {
            match &source.poll {
                Polling::One(poll) => {
                    let outcome = sweep(flow, source, poll.as_ref(), http, run)
                        .await
                        .unwrap_or_else(Sweep::Stopped);
                    vec![Swept {
                        source: source.name.clone(),
                        member: None,
                        outcome,
                    }]
                }
                Polling::Many(expand) => {
                    members::swept(flow, source, expand.as_ref(), http, run).await
                }
            }
        }))
        .buffered(at_once(ports.store, config.run.sources).get())
        .collect()
        .await;
    swept.into_iter().flatten().collect()
}

mod members;
