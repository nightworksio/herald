//! Each report whose window began and was not yet announced: its view's
//! open pull requests read from every source it names, and announced
//! through the routes naming the report (ADR-0055).

use herald_config::{Config, Report};
use herald_core::{
    Cursor, Identity, Pull, RouteId, Showing, SourceName, Visibility, board, report_event,
};
use herald_ports::{Clock, Cursors, Http, Signer, Store, Versioned};
use jiff::Timestamp;

use crate::announce::{Outcome, Ports, Stop, Within, announce_within};
use crate::poll::{Fresh, Remembered, Sweep, Swept, halted, remembered, stored};
use crate::prepare::Connected;

/// Why a report's view could not be read: a source it names is not one
/// this run reads.
const UNREAD_SOURCE: &str = "the view reads a source this run does not poll";

/// The open pull requests of every source `report`'s view reads, the
/// first that could not be read stopping it.
async fn read_pulls<S: Store, C: Clock, G: Signer, H: Http>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    report: &Report,
) -> Result<Vec<Pull>, Stop> {
    let mut pulls = Vec::new();
    let Some(view) = config.views.get(&report.view) else {
        return Ok(pulls);
    };
    for name in &view.sources {
        let Some((queue, http)) = connected
            .sources
            .iter()
            .find(|(source, _)| source.name == *name)
            .and_then(|(source, http)| source.queue.as_ref().map(|queue| (queue, http)))
        else {
            return Err(Stop::Source(UNREAD_SOURCE.to_owned()));
        };
        ports.budget.allows(ports.clock.now(), 1)?;
        let read = queue.opened(http, view.most).await.map_err(halted)?;
        pulls.extend(read);
    }
    Ok(pulls)
}

/// What one announcement of a report shows: the pull requests, who may see
/// them, and the routes it may reach, where only some.
struct Part<'a> {
    pulls: &'a [Pull],
    visibility: Visibility,
    routes: Option<&'a [RouteId]>,
}

/// `report`'s window that began at `start`, showing `part`, announced to
/// the routes naming the report among those `part` may reach.
async fn announced_part<S: Store, C: Clock, G: Signer, H: Http>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (name, report, start): (&str, &Report, Timestamp),
    part: Part<'_>,
    run: &str,
) -> Result<Fresh, Stop> {
    let view = config.views.get(&report.view).map(|view| &view.view);
    let shown = board(
        view.unwrap_or(&herald_core::View::default()),
        part.pulls,
        ports.clock.now(),
    );
    let event = report_event(name, &report.view, &shown, start)
        .map_err(|problem| Stop::Source(problem.to_string()))?;
    let seen = event
        .visibility()
        .unwrap_or(Visibility::Private)
        .max(part.visibility);
    let event = event.seen_as(seen);
    let within = Within {
        routes: part.routes,
        view: Some(Showing {
            view: &report.view,
            board: &shown,
        }),
        ..Within::default()
    };
    let source = SourceName::new(name);
    let announced = announce_within(config, connected, ports, &source, &event, within, run).await;
    Ok(Fresh {
        event,
        announced,
        held: None,
    })
}

/// The report `name`, where its window began and was not yet announced:
/// where `[doctor] private_to_public` refuses and it read a repository
/// not everyone may see, its private routes are shown every row and the
/// rest only the public repositories' rows, and nothing where it read no
/// public repository's.
async fn reported_one<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (name, report): (&str, &Report),
    run: &str,
) -> Option<Sweep> {
    let (config, _, ports) = flow;
    let now = ports.clock.now();
    let start = report.schedule.window(now)?;
    let source = SourceName::new(name);
    let record = match ports.store.cursors().cursor(&source).await {
        Ok(record) => record,
        Err(error) => return Some(Sweep::Stopped(stored(error))),
    };
    let identity = Identity::new(herald_core::report_identity(name, start));
    let cursor = record
        .as_ref()
        .map(|record: &Versioned<Cursor>| &record.value);
    if cursor.is_some_and(|cursor| cursor.seen.first() == Some(&identity)) {
        return None;
    }
    let pulls = match read_pulls(flow, report).await {
        Ok(pulls) => pulls,
        Err(stop) => return Some(Sweep::Stopped(stop)),
    };
    let strictest = pulls
        .iter()
        .map(|pull| pull.visibility)
        .max()
        .unwrap_or(Visibility::Public);
    let audiences = match crate::poll::bounded(config, strictest) {
        None => vec![(pulls.clone(), strictest, None)],
        Some(private) => {
            let public: Vec<RouteId> = config
                .routes
                .iter()
                .map(|route| route.route.id.clone())
                .filter(|id| !private.contains(id))
                .collect();
            let open: Vec<Pull> = pulls
                .iter()
                .filter(|pull| pull.visibility == Visibility::Public)
                .cloned()
                .collect();
            let shown = (!open.is_empty()).then_some((open, Visibility::Public, Some(public)));
            std::iter::once((pulls.clone(), strictest, Some(private)))
                .chain(shown)
                .collect()
        }
    };
    let mut announced = Vec::new();
    for (shown, visibility, routes) in &audiences {
        let part = Part {
            pulls: shown,
            visibility: *visibility,
            routes: routes.as_deref(),
        };
        match announced_part(flow, (name, report, start), part, run).await {
            Ok(fresh) => announced.push(fresh),
            Err(stop) => return Some(Sweep::Stopped(stop)),
        }
    }
    let settled = !announced
        .iter()
        .flat_map(|fresh| &fresh.announced)
        .any(|announced| matches!(announced.outcome, Outcome::Stopped { .. }));
    let cursor = if settled {
        let advanced = Cursor {
            seen: vec![identity],
            ..Cursor::default()
        };
        let expected = record.as_ref().map(|record| record.version);
        let written = ports
            .store
            .cursors()
            .put_cursor(&source, &advanced, expected)
            .await;
        remembered(written, "the report's cursor")
    } else {
        Remembered::Kept
    };
    Some(Sweep::Reported {
        pulls: pulls.len(),
        announced,
        cursor,
    })
}

/// Every report of `config` whose window began and was not yet announced,
/// announced once every route has it.
pub(crate) async fn reported<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    run: &str,
) -> Vec<Swept> {
    let mut swept = Vec::new();
    for (name, report) in &flow.0.reports {
        if let Some(outcome) = reported_one(flow, (name, report), run).await {
            swept.push(Swept {
                source: SourceName::new(name),
                member: None,
                outcome,
            });
        }
    }
    swept
}

/// The report round: each installation's token minted for it, then every
/// report whose window began announced, as [`crate::poll`] does after its
/// sources (ADR-0055).
pub async fn report<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    run: &str,
) -> Vec<Swept> {
    let flow = (config, connected, ports);
    let (_, mut swept) = crate::poll::minting(flow, |_| true).await;
    swept.extend(reported(flow, run).await);
    swept
}
