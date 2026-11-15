//! A tag held back until the Release of its version comes or its window
//! passes (ADR-0032).

use herald_config::Config;
use herald_core::{Event, HeldEvent, Hold, Pending, Signal, SourceName};
use herald_ports::{Clock, Cursors, Http, Signer, Store, Versioned, Waiting};
use jiff::{SignedDuration, Timestamp};

use super::{Fresh, Sweep, Swept, bounded, stored};
use crate::announce::{Outcome, Ports, Stop, announce_within};
use crate::prepare::Connected;

/// How a source's tags wait for their Release: as long as `window`, each
/// held as of the owner's member `member`, where it is one's.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Holding<'a> {
    pub(crate) window: SignedDuration,
    pub(crate) member: Option<&'a str>,
}

/// Why a held tag of an owner's member is let go unannounced.
const UNLISTED: &str = "the owner's last membership record no longer holds the member";

/// When `event`, found fresh by `source` holding as `holding` says, is
/// announced, where it waits: a tag waits for its Release, held from now on
/// where nothing holds it yet; anything else, as its Release, lets the tag
/// of its identity go, and is announced at once.
pub(crate) async fn held<S: Store, C: Clock, G>(
    ports: &Ports<'_, S, C, G>,
    (source, holding): (&SourceName, Holding<'_>),
    event: &Event,
) -> Result<Option<Timestamp>, Stop> {
    let hold = Hold::Tag {
        source: source.clone(),
        identity: event.identity().clone(),
    };
    let waiting = ports.store.waiting();
    if event.signal() != Some(Signal::Tag) {
        waiting.drop_pending(&hold).await.map_err(stored)?;
        return Ok(None);
    }
    if let Some(kept) = waiting.pending(&hold).await.map_err(stored)? {
        return Ok(Some(kept.value.due));
    }
    let opened = ports.clock.now();
    let due = opened.checked_add(holding.window).unwrap_or(Timestamp::MAX);
    let pending = Pending {
        hold,
        events: vec![HeldEvent {
            source: source.clone(),
            member: holding.member.map(ToOwned::to_owned),
            event: event.clone(),
        }],
        opened,
        due,
    };
    waiting.put_pending(&pending, None).await.map_err(stored)?;
    Ok(Some(due))
}

/// `event`, a tag or Release `source` names, held for its Release where the
/// source holds its tags, and otherwise announced to every route that takes
/// it under `run`: what a CI run for a tag or a release does (ADR-0032).
///
/// # Errors
///
/// Where its held record cannot be read or written.
pub async fn held_or_announced<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    (source, event): (&SourceName, &Event),
    run: &str,
) -> Result<Fresh, Stop> {
    let window = config
        .sources
        .get(source)
        .and_then(|options| options.tag_window());
    if let Some(window) = window
        && let Some(due) = held(
            ports,
            (
                source,
                Holding {
                    window,
                    member: None,
                },
            ),
            event,
        )
        .await?
    {
        return Ok(Fresh {
            event: event.clone(),
            announced: Vec::new(),
            held: Some(due),
        });
    }
    let announced = announce_within(config, connected, ports, source, event, None, run).await;
    Ok(Fresh {
        event: event.clone(),
        announced,
        held: None,
    })
}

/// Every held event whose window has passed announced, each to the routes
/// that take it, as its source's under `run`, and its record dropped once
/// every route has it; a tag of an owner's member the owner's last
/// membership record no longer holds is let go unannounced. A store that
/// keeps no held events holds none to announce.
///
/// # Errors
///
/// Where the held events cannot be read.
pub async fn announce_held<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    run: &str,
) -> Result<Vec<Swept>, Stop> {
    let Some(waiting) = ports.store.waiting() else {
        return Ok(Vec::new());
    };
    let every = waiting.every_pending().await.map_err(stored)?;
    let now = ports.clock.now();
    let mut swept = Vec::new();
    for Versioned { value: pending, .. } in every.into_iter().filter(|kept| kept.value.due_at(now))
    {
        let mut done = Vec::new();
        for held in &pending.events {
            done.push(announced(config, connected, ports, held, run).await);
        }
        let settled = done.iter().all(|swept| match &swept.outcome {
            Sweep::Released { announced, .. } => announced.iter().all(|fresh| {
                fresh
                    .announced
                    .iter()
                    .all(|announced| !matches!(announced.outcome, Outcome::Stopped { .. }))
            }),
            _ => false,
        });
        swept.extend(done);
        if settled && let Err(error) = waiting.drop_pending(&pending.hold).await {
            let Hold::Tag { source, .. } = &pending.hold;
            swept.push(Swept {
                source: source.clone(),
                member: None,
                outcome: Sweep::Stopped(stored(error)),
            });
        }
    }
    Ok(swept)
}

/// `held` announced to the routes that take it, bounded as its owner's
/// member is where it is one's.
async fn announced<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    held: &HeldEvent,
    run: &str,
) -> Swept {
    let source = &held.source;
    let swept = |member, outcome| Swept {
        source: source.clone(),
        member,
        outcome,
    };
    let (path, within) = match &held.member {
        None => (None, None),
        Some(id) => match ports.store.cursors().member(source, id).await {
            Err(error) => return swept(None, Sweep::Stopped(stored(error))),
            Ok(Some(kept)) if kept.value.left.is_none() => (
                Some(kept.value.repository.path.clone()),
                bounded(config, kept.value.repository.visibility),
            ),
            Ok(_) => {
                return swept(
                    None,
                    Sweep::Released {
                        announced: Vec::new(),
                        withheld: Some(UNLISTED.to_owned()),
                    },
                );
            }
        },
    };
    let holder = format!("{run}/{}", source.as_str());
    let outcomes = announce_within(
        config,
        connected,
        ports,
        source,
        &held.event,
        within.as_deref(),
        &holder,
    )
    .await;
    swept(
        path,
        Sweep::Released {
            announced: vec![Fresh {
                event: held.event.clone(),
                announced: outcomes,
                held: None,
            }],
            withheld: None,
        },
    )
}
