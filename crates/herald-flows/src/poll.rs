//! Each source asked for what it has not seen, and each event it finds
//! announced (ADR-0003).

use futures_util::stream::{self, StreamExt};
use herald_config::Config;
use herald_core::{Event, SourceName, fresh};
use herald_kinds::Unpolled;
use herald_ports::{Clock, Cursors, Http, Signer, Store, Written};

use crate::announce::{Announced, Outcome, Ports, Stop, announce, at_once};
use crate::prepare::{Connected, PreparedSource};

/// What polling one source did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Swept {
    /// The source.
    pub source: SourceName,
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

fn stored(error: impl std::fmt::Display) -> Stop {
    Stop::Store(error.to_string())
}

async fn sweep<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    (source, http): &(PreparedSource, H),
    run: &str,
) -> Result<Sweep, Stop> {
    let record = ports
        .store
        .cursors()
        .cursor(&source.name)
        .await
        .map_err(stored)?;
    let cursor = record.as_ref().map(|record| &record.value);
    let polled = source
        .poll
        .polled(http, cursor)
        .await
        .map_err(|unpolled| match unpolled {
            Unpolled::Unreachable(reason) => Stop::Unreachable(reason),
            Unpolled::Unreadable(reason) => Stop::Source(reason),
        })?;
    let Some(releases) = polled else {
        return Ok(Sweep::Unchanged);
    };
    let found = releases.events.len();
    let holder = format!("{run}/{}", source.name.as_str());
    let mut announced = Vec::new();
    let mut settled = true;
    for event in fresh(cursor, releases.events.clone(), source.backfill)
        .into_iter()
        .rev()
    {
        let outcomes = announce(config, connected, ports, &source.name, &event, &holder).await;
        settled &= !outcomes
            .iter()
            .any(|outcome| matches!(outcome.outcome, Outcome::Stopped { .. }));
        announced.push(Fresh {
            event,
            announced: outcomes,
        });
    }
    let remembered = if settled {
        let advanced = cursor.cloned().unwrap_or_default().advanced(
            &releases.events,
            releases.etag,
            releases.last_modified,
            source.poll.remembered(),
        );
        let expected = record.as_ref().map(|record| record.version);
        match ports
            .store
            .cursors()
            .put_cursor(&source.name, &advanced, expected)
            .await
        {
            Ok(Written::Written(_)) => Remembered::Advanced,
            Ok(Written::Conflict) => Remembered::Failed(Stop::Store(
                "another run moved the source's cursor first".to_owned(),
            )),
            Err(error) => Remembered::Failed(stored(error)),
        }
    } else {
        Remembered::Kept
    };
    Ok(Sweep::Polled {
        first: record.is_none(),
        found,
        announced,
        cursor: remembered,
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
    stream::iter(connected.sources.iter().map(|source| async move {
        let outcome = sweep(config, connected, ports, source, run)
            .await
            .unwrap_or_else(Sweep::Stopped);
        Swept {
            source: source.0.name.clone(),
            outcome,
        }
    }))
    .buffered(at_once(ports.store, config.run.sources))
    .collect()
    .await
}
