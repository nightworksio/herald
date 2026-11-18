//! Each source asked for what it has not seen, and each event it finds
//! announced (ADR-0003).

use futures_util::stream::{self, StreamExt};
use herald_config::Config;
use std::collections::BTreeMap;
use std::iter::once;

use herald_core::{Cursor, Event, Identity, Polled, SourceName, fresh};
use herald_kinds::{Expand, Poll, Polling, Pull, Unpolled};
use herald_ports::{
    Clock, Cursors, DynVerifier, Http, Signer, Store, StoreError, Versioned, Written,
};
use jiff::Timestamp;

use crate::announce::{Announced, Outcome, Ports, Stop, Within, announce_within, at_once};
use crate::installations;
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
    /// An event held back until it was due was announced, or let go
    /// unannounced, as this says (ADR-0032).
    Released {
        /// The event announced, and what every route did with it.
        announced: Vec<Fresh>,
        /// Why it was let go unannounced, where it was.
        withheld: Option<String>,
    },
    /// A release the forge deleted, whose withdrawal a run stopped
    /// part-way, was withdrawn by a round, or stopped again (ADR-0048); or
    /// one another herald's feed withdrew by a tombstone, by the poll that
    /// read it (ADR-0049).
    Withdrawn {
        /// The release.
        identity: Identity,
        /// What its route did.
        announced: Vec<Announced>,
    },
    /// A pull took no new identity of the instance it names, as the source
    /// keeps as many tombstones of its releases as `peer_share` lets it, and
    /// reads that envelope again in a later pull (ADR-0049).
    Refused(String),
    /// A pull passed an envelope of the instance it names over, as its
    /// proof was made further ahead of the round than any may be
    /// (ADR-0049).
    Ahead(String),
    /// A report's window began, and its view was announced (ADR-0055); or
    /// a rule's, and its nudges were (ADR-0056).
    Reported {
        /// How many open pull requests its view's sources listed.
        pulls: usize,
        /// The report as each route was shown it, and what every route did
        /// with it: every row to a private route, and where `[doctor]
        /// private_to_public` refuses, only the public repositories' rows to
        /// the rest; or each nudge, and what every route did with it.
        announced: Vec<Fresh>,
        /// What became of the report's or rule's cursor, which remembers
        /// the window once every route has it.
        cursor: Remembered,
    },
    /// The installation token the source reads with was minted for the
    /// round but not kept in the store, for this reason; the next run
    /// mints it again (ADR-0041).
    Unkept(String),
}

/// An event a poll announced, and what every route did with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fresh {
    /// The event.
    pub event: Event,
    /// What every route that takes it did with it.
    pub announced: Vec<Announced>,
    /// When it is announced, where it is a tag held back for its Release
    /// (ADR-0032).
    pub held: Option<Timestamp>,
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
    pub(crate) withdrawn: Vec<Sweep>,
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
    (source, within): (&SourceName, Within<'_>),
    (poll, http): (&dyn Poll, &H),
    (cursor, since, backfill): (Option<&Cursor>, Option<Timestamp>, usize),
    holder: &str,
) -> Result<Option<Found>, Stop> {
    let polled = poll
        .polled(http, cursor, ports.clock.now())
        .await
        .map_err(halted)?;
    let holding = poll.tag_window().map(|window| Holding {
        window,
        member: None,
    });
    announced_found(
        (config, connected, ports),
        (source, within, holding),
        (polled, poll.remembered()),
        (cursor, since, backfill),
        holder,
    )
    .await
}

/// What `polled`, a poll's answer since `cursor`, holds that it has not
/// seen, each event announced as `source`'s under `holder` within `within`,
/// with the cursor to remember, holding `remembered` identities, where
/// every route has its events; nothing where it says nothing changed.
pub(crate) async fn announced_found<S: Store, C: Clock, G: Signer, H: Http>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (source, within, holding): (&SourceName, Within<'_>, Option<Holding<'_>>),
    (polled, remembered): (Option<Polled>, usize),
    (cursor, since, backfill): (Option<&Cursor>, Option<Timestamp>, usize),
    holder: &str,
) -> Result<Option<Found>, Stop> {
    let Some(releases) = polled else {
        return Ok(None);
    };
    let fresh_events = unseen(cursor, &releases.events, since, backfill);
    let mut announced = Vec::new();
    let mut settled = true;
    for event in fresh_events.into_iter().rev() {
        if let Some(holding) = holding
            && let Some(due) = held(ports, (source, holding), &event).await?
        {
            announced.push(Fresh {
                event,
                announced: Vec::new(),
                held: Some(due),
            });
            continue;
        }
        let outcomes =
            announce_within(config, connected, ports, source, &event, within, holder).await;
        settled &= !outcomes
            .iter()
            .any(|outcome| matches!(outcome.outcome, Outcome::Stopped { .. }));
        announced.push(Fresh {
            event,
            announced: outcomes,
            held: None,
        });
    }
    let advanced = settled.then(|| {
        cursor
            .cloned()
            .unwrap_or_default()
            .advanced(&releases, remembered)
    });
    Ok(Some(Found {
        count: releases.events.len(),
        announced,
        advanced,
        withdrawn: Vec::new(),
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

/// What a source polled as one is read through: its own poll, or another
/// herald's feed with what checks its signatures.
#[derive(Clone, Copy)]
enum Reader<'a> {
    Poll(&'a dyn Poll),
    Pull(&'a dyn Pull, &'a dyn DynVerifier),
}

async fn sweep<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (source, record): (
        &PreparedSource,
        Result<Option<Versioned<Cursor>>, StoreError>,
    ),
    reader: Reader<'_>,
    http: &H,
    run: &str,
) -> Result<Vec<Sweep>, Stop> {
    let ports = flow.2;
    ports.budget.allows(ports.clock.now(), 1)?;
    let record = record.map_err(stored)?;
    let cursor = record.as_ref().map(|record| &record.value);
    let holder = format!("{run}/{}", source.name.as_str());
    let read = match reader {
        Reader::Poll(poll) => {
            found(
                flow,
                (&source.name, Within::default()),
                (poll, http),
                (cursor, None, source.backfill),
                &holder,
            )
            .await?
        }
        Reader::Pull(pull, verifier) => {
            pulled::found(
                flow,
                (&source.name, Within::default()),
                (pull, verifier, http),
                (cursor, source.backfill),
                &holder,
            )
            .await?
        }
    };
    let Some(found) = read else {
        return Ok(vec![Sweep::Unchanged]);
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
    let polled = Sweep::Polled {
        first: record.is_none(),
        found: found.count,
        announced: found.announced,
        cursor,
    };
    Ok(once(polled).chain(found.withdrawn).collect())
}

/// The round of `source`, polled as one through `reader`, with its cursor
/// as read, the project it last claimed noted in `claims`.
async fn one<'a, S: Store, C: Clock, G: Signer>(
    ports: &Ports<'_, S, C, G>,
    claims: &mut Claims,
    source: &PreparedSource,
    reader: Reader<'a>,
) -> Round<'a> {
    let read = ports.store.cursors().cursor(&source.name).await;
    match &read {
        Ok(record) => {
            if let Some(project) = record
                .as_ref()
                .and_then(|record| record.value.project.as_deref())
            {
                claims.source(&source.name, project);
            }
        }
        Err(_) => claims.unread(&source.name),
    }
    Round::One(reader, Box::new(read))
}

/// One source's round, once every owner is reconciled: a source polled
/// as one, with its cursor as read, or an owner's members.
enum Round<'a> {
    One(
        Reader<'a>,
        Box<Result<Option<Versioned<Cursor>>, StoreError>>,
    ),
    Many(&'a dyn Expand, Result<Box<members::Owning>, Vec<Swept>>),
}

/// Every source of `connected` polled, and each event it has not seen
/// announced to every route that takes it (ADR-0003). A source's cursor
/// remembers what it found only once every route has its events, so a
/// route that stopped is offered them again by the next poll. As many
/// sources as `[run] sources` says are polled at once, one at a time where
/// the store writes one record at a time (ADR-0025). Each source takes its
/// leases under `run` and its own name, so two sources finding one event
/// never both send it.
/// A source pulling another herald's feed checks its answers through
/// `verifier` (ADR-0049). Then each report whose window began is announced
/// once (ADR-0055).
pub async fn poll<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    verifier: &dyn DynVerifier,
    run: &str,
) -> Vec<Swept> {
    let mut swept = polled(config, connected, ports, verifier, (run, None)).await;
    swept.extend(crate::report::reported((config, connected, ports), run).await);
    swept
}

/// What the source `source` did, as `outcome` says.
fn swept(source: &SourceName, outcome: Sweep) -> Swept {
    Swept {
        source: source.clone(),
        member: None,
        outcome,
    }
}

/// Each installation the sources read given a token for the round: why the
/// sources reading one that could not be minted are not polled, and the
/// sweeps of those `polling` sweeps, with a note for each source whose
/// token the store did not keep.
pub(crate) async fn minting<S: Store, C: Clock, G: Signer, H: Http>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    polling: impl Fn(&SourceName) -> bool,
) -> (BTreeMap<SourceName, Stop>, Vec<Swept>) {
    let (unminted, unkept) =
        installations::refreshed(&connected.mintings, ports, config.store.kind()).await;
    let stopped = unminted
        .iter()
        .map(|(source, stop)| (source, Sweep::Stopped(stop.clone())));
    let noted = unkept.iter().flat_map(|unkept| {
        unkept
            .sources
            .iter()
            .map(|source| (source, Sweep::Unkept(unkept.why.clone())))
    });
    let swept = stopped
        .chain(noted)
        .filter(|(source, _)| polling(source))
        .map(|(source, outcome)| swept(source, outcome))
        .collect();
    (unminted, swept)
}

/// Every held event whose window passed, released; where the held events
/// cannot be read, each source that holds them stopped.
async fn released_held<S: Store, C: Clock, G: Signer, H: Http>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    run: &str,
) -> Vec<Swept> {
    match held::announce_held(config, connected, ports, run).await {
        Ok(released) => released,
        Err(stop) => config
            .sources
            .iter()
            .filter(|(_, options)| options.holds())
            .map(|(source, _)| swept(source, Sweep::Stopped(stop.clone())))
            .collect(),
    }
}

/// [`poll`], sweeping the source `only` alone where it names one, as a
/// webhook of a tag asks (ADR-0032); every owner is still listed, so each
/// member's project is checked against every other's.
pub(crate) async fn polled<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    verifier: &dyn DynVerifier,
    (run, only): (&str, Option<&SourceName>),
) -> Vec<Swept> {
    let flow = (config, connected, ports);
    let polling = |name: &SourceName| only.is_none_or(|only| only == name);
    let mut released = released_held(flow, run).await;
    let (unminted, noted) = minting(flow, polling).await;
    released.extend(noted);
    let minted = |source: &PreparedSource| !unminted.contains_key(&source.name);
    let at_once = at_once(ports.store, config.run.sources).get();
    let owners: Vec<(&SourceName, Result<members::Owning, Vec<Swept>>)> = stream::iter(
        connected
            .sources
            .iter()
            .filter(|(source, _)| minted(source))
            .filter_map(|(source, http)| match &source.poll {
                Polling::Many(expand) => Some((source, expand, http)),
                Polling::One(_) | Polling::Pull(_) | Polling::Unpolled => None,
            })
            .map(|(source, expand, http)| async move {
                (
                    &source.name,
                    members::reconciled(flow, (source, expand.as_ref(), http)).await,
                )
            }),
    )
    .buffered(at_once)
    .collect()
    .await;
    let mut owners: BTreeMap<&SourceName, Result<members::Owning, Vec<Swept>>> =
        owners.into_iter().collect();
    let mut claims = Claims::of(config);
    let mut rounds = Vec::new();
    for (source, http) in &connected.sources {
        if !minted(source) {
            claims.unread(&source.name);
            continue;
        }
        let round = match &source.poll {
            Polling::One(poll) => {
                one(ports, &mut claims, source, Reader::Poll(poll.as_ref())).await
            }
            Polling::Pull(pull) => {
                one(
                    ports,
                    &mut claims,
                    source,
                    Reader::Pull(pull.as_ref(), verifier),
                )
                .await
            }
            Polling::Unpolled => continue,
            Polling::Many(expand) => {
                let owning = owners.remove(&source.name).unwrap_or(Err(Vec::new()));
                match &owning {
                    Ok(owning) => claims.members((&source.name, expand.as_ref()), owning.taken()),
                    Err(_) => claims.unread(&source.name),
                }
                Round::Many(expand.as_ref(), owning.map(Box::new))
            }
        };
        if polling(&source.name) {
            rounds.push((source, http, round));
        }
    }
    let claims = &claims;
    let swept: Vec<Vec<Swept>> =
        stream::iter(rounds.into_iter().map(|(source, http, round)| async move {
            match round {
                Round::One(reader, cursor) => sweep(flow, (source, *cursor), reader, http, run)
                    .await
                    .unwrap_or_else(|stop| vec![Sweep::Stopped(stop)])
                    .into_iter()
                    .map(|outcome| swept(&source.name, outcome))
                    .collect(),
                Round::Many(expand, Ok(owning)) => {
                    members::finished(flow, (source, expand, http), (*owning, claims), run).await
                }
                Round::Many(_, Err(stopped)) => stopped,
            }
        }))
        .buffered(at_once)
        .collect()
        .await;
    released.extend(swept.into_iter().flatten());
    released
}

mod claims;
mod held;
mod members;
pub(crate) mod pulled;

use claims::Claims;
use held::{Holding, held};

pub use held::{announce_held, held_or_announced};

pub(crate) use members::bounded;
