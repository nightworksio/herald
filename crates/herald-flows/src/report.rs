//! Each report whose window began and was not yet announced: its view's
//! open pull requests read from every source it names, and announced
//! through the routes naming the report (ADR-0055).

use std::collections::BTreeMap;

use herald_config::{Config, Report, ViewConfig};
use herald_core::{
    Board, Cursor, Identity, Pull, RouteId, Row, Showing, SourceName, Visibility, grouped,
    report_event,
};
use herald_ports::{Clock, Cursors, Http, Signer, Store, Versioned};
use jiff::Timestamp;

use crate::announce::{Outcome, Ports, Stop, Within, announce_within};
use crate::deciding::decided_rows;
use crate::poll::{Fresh, Remembered, Sweep, Swept, halted, remembered, stored};
use crate::prepare::Connected;

/// Why a report's view could not be read: a source it names is not one
/// this run reads.
const UNREAD_SOURCE: &str = "the view reads a source this run does not poll";

/// The open pull requests each view a round read, by the view's name, so
/// a report and a rule watching one view read it once a round.
pub(crate) type Reads = BTreeMap<String, Vec<Pull>>;

/// The open pull requests of the view `name`, read once a round into
/// `reads`.
pub(crate) async fn view_pulls<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (name, view): (&str, &ViewConfig),
    reads: &mut Reads,
) -> Result<Vec<Pull>, Stop> {
    if let Some(pulls) = reads.get(name) {
        return Ok(pulls.clone());
    }
    let pulls = read_pulls(flow, view).await?;
    reads.insert(name.to_owned(), pulls.clone());
    Ok(pulls)
}

/// The open pull requests of every source `view` reads, the first that
/// could not be read stopping it.
async fn read_pulls<S: Store, C: Clock, G: Signer, H: Http>(
    (_, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    view: &ViewConfig,
) -> Result<Vec<Pull>, Stop> {
    let mut pulls = Vec::new();
    for name in &view.sources {
        let (queue, http) = connected
            .sources
            .iter()
            .find(|(source, _)| source.name == *name)
            .and_then(|(source, http)| source.queue.as_ref().map(|queue| (queue, http)))
            .ok_or_else(|| Stop::Source(UNREAD_SOURCE.to_owned()))?;
        ports.budget.allows(ports.clock.now(), 1)?;
        let read = queue.opened(http, view.most).await.map_err(halted)?;
        pulls.extend(read);
    }
    Ok(pulls)
}

/// The board of the view `name`, its sources' open pull requests read now
/// with each installation's token minted first, as `herald queue` shows
/// it (ADR-0055).
///
/// # Errors
///
/// Where the config has no such view, or a source it reads could not be
/// read.
pub async fn queue<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    name: &str,
) -> Result<Board, Stop> {
    let Some(view) = config.views.get(name) else {
        let names: Vec<String> = config
            .views
            .keys()
            .map(|name| format!("\"{name}\""))
            .collect();
        return Err(Stop::Source(format!(
            "no view is named \"{name}\"; the views are {}",
            names.join(", ")
        )));
    };
    let flow = (config, connected, ports);
    crate::poll::minting(flow, |_| true).await;
    let pulls = read_pulls(flow, view).await?;
    Ok(grouped(&view.view, decided_rows(flow, view, &pulls).await))
}

/// What one announcement of a report shows: the pull requests, who may see
/// them, the routes it may reach, where only some, and whether those keep
/// one living post of it.
struct Part<'a> {
    view: &'a ViewConfig,
    rows: &'a [Row],
    visibility: Visibility,
    routes: &'a [RouteId],
    living: bool,
}

/// `report`'s window that began at `start`, showing `part`, announced to
/// the routes naming the report among those `part` may reach.
async fn announced_part<S: Store, C: Clock, G: Signer, H: Http>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (name, report, start): (&str, &Report, Timestamp),
    part: Part<'_>,
    run: &str,
) -> Result<Fresh, Stop> {
    let shown = grouped(&part.view.view, part.rows.to_vec());
    let event = report_event((name, &report.view), &shown, start, part.living)
        .map_err(|problem| Stop::Source(problem.to_string()))?
        .seen_as(part.visibility);
    let within = Within {
        routes: Some(part.routes),
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

/// The window that began at `start`, where the cursor of the report or
/// rule `name` has not yet remembered it: its identity, and the cursor as
/// the store keeps it.
pub(crate) async fn due<S: Store, C: Clock, G: Signer>(
    ports: &Ports<'_, S, C, G>,
    name: &str,
    start: Timestamp,
) -> Result<Option<(Identity, Option<Versioned<Cursor>>)>, Stop> {
    let record = ports
        .store
        .cursors()
        .cursor(&SourceName::new(name))
        .await
        .map_err(stored)?;
    let identity = Identity::new(herald_core::report_identity(name, start));
    let seen = record
        .as_ref()
        .is_some_and(|record| record.value.seen.first() == Some(&identity));
    Ok((!seen).then_some((identity, record)))
}

/// What became of the cursor of the report or rule `name` once `announced`
/// went out: the window `identity` remembered over `record` where no route
/// stopped, and kept where one did.
pub(crate) async fn settled<S: Store, C: Clock, G: Signer>(
    ports: &Ports<'_, S, C, G>,
    (name, identity): (&str, Identity),
    record: Option<&Versioned<Cursor>>,
    announced: &[Fresh],
) -> Remembered {
    let stopped = announced
        .iter()
        .flat_map(|fresh| &fresh.announced)
        .any(|announced| matches!(announced.outcome, Outcome::Stopped { .. }));
    if stopped {
        return Remembered::Kept;
    }
    let advanced = Cursor {
        seen: vec![identity],
        ..Cursor::default()
    };
    let written = ports
        .store
        .cursors()
        .put_cursor(
            &SourceName::new(name),
            &advanced,
            record.map(|record| record.version),
        )
        .await;
    remembered(written, "the cursor")
}

/// The routes of `connected` that keep a living post of a report: those
/// that ask to and reach a destination that edits.
fn living<H>(connected: &Connected<H>) -> Vec<RouteId> {
    connected
        .routes
        .iter()
        .filter(|(route, _)| {
            route.config.route.delivery.living && route.reached.profile().features.edits
        })
        .map(|(route, _)| route.config.route.id.clone())
        .collect()
}

/// Who of the routes of `config` naming the report `name` is shown which
/// of `pulls`, the strictest of them seen as `strictest`: every row to
/// those `private` names, or to all where it names none, and the public
/// repositories' rows to the rest where there are any; each audience
/// parted into the routes that keep a living post and those that do not.
fn audiences(
    (config, name): (&Config, &str),
    rows: &[Row],
    (strictest, private): (Visibility, Option<Vec<RouteId>>),
    living: &[RouteId],
) -> Vec<(Vec<Row>, Visibility, Vec<RouteId>, bool)> {
    let every: Vec<RouteId> = config
        .routes
        .iter()
        .filter(|route| {
            route
                .route
                .filter
                .sources
                .iter()
                .any(|source| source.as_str() == name)
        })
        .map(|route| route.route.id.clone())
        .collect();
    let shown = match private {
        None => vec![(rows.to_vec(), strictest, every)],
        Some(private) => {
            let public: Vec<RouteId> = every
                .iter()
                .filter(|id| !private.contains(id))
                .cloned()
                .collect();
            let open: Vec<Row> = rows
                .iter()
                .filter(|row| row.pull.visibility == Visibility::Public)
                .cloned()
                .collect();
            let open = (!open.is_empty()).then_some((open, Visibility::Public, public));
            let private = private
                .into_iter()
                .filter(|id| every.contains(id))
                .collect();
            std::iter::once((rows.to_vec(), strictest, private))
                .chain(open)
                .collect()
        }
    };
    shown
        .into_iter()
        .flat_map(|(rows, visibility, routes)| {
            let (lives, posts): (Vec<RouteId>, Vec<RouteId>) =
                routes.into_iter().partition(|id| living.contains(id));
            [
                (rows.clone(), visibility, lives, true),
                (rows, visibility, posts, false),
            ]
        })
        .filter(|(_, _, routes, _)| !routes.is_empty())
        .collect()
}

/// The report `name`, where its window began and was not yet announced:
/// where `[doctor] private_to_public` refuses and it read a repository
/// not everyone may see, its private routes are shown every row and the
/// rest only the public repositories' rows, and nothing where it read no
/// public repository's; a living route is shown it as its one post.
async fn reported_one<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (name, report): (&str, &Report),
    run: &str,
    reads: &mut Reads,
) -> Option<Sweep> {
    let (config, connected, _) = flow;
    let start = report.schedule.window(flow.2.clock.now())?;
    let (identity, record) = match due(flow.2, name, start).await {
        Ok(due) => due?,
        Err(stop) => return Some(Sweep::Stopped(stop)),
    };
    let view = config.views.get(&report.view)?;
    let pulls = match view_pulls(flow, (&report.view, view), reads).await {
        Ok(pulls) => pulls,
        Err(stop) => return Some(Sweep::Stopped(stop)),
    };
    let strictest = pulls
        .iter()
        .map(|pull| pull.visibility)
        .max()
        .unwrap_or(Visibility::Public);
    let private = crate::poll::bounded(config, strictest);
    let decided = decided_rows(flow, view, &pulls).await;
    let mut announced = Vec::new();
    for (shown, visibility, routes, living) in audiences(
        (config, name),
        &decided,
        (strictest, private),
        &living(connected),
    ) {
        let part = Part {
            view,
            rows: &shown,
            visibility,
            routes: &routes,
            living,
        };
        match announced_part(flow, (name, report, start), part, run).await {
            Ok(fresh) => announced.push(fresh),
            Err(stop) => return Some(Sweep::Stopped(stop)),
        }
    }
    let cursor = settled(flow.2, (name, identity), record.as_ref(), &announced).await;
    Some(Sweep::Reported {
        pulls: pulls.len(),
        announced,
        cursor,
    })
}

/// Every report of `config` whose window began and was not yet announced,
/// announced once every route has it, then every rule's nudges, each view
/// read once.
pub(crate) async fn reported<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    run: &str,
) -> Vec<Swept> {
    let mut swept = Vec::new();
    let mut reads = Reads::new();
    for (name, report) in &flow.0.reports {
        if let Some(outcome) = reported_one(flow, (name, report), run, &mut reads).await {
            swept.push(Swept {
                source: SourceName::new(name),
                member: None,
                outcome,
            });
        }
    }
    for (name, rule) in &flow.0.rules {
        if let Some(outcome) = crate::rules::nudged(flow, (name, rule), run, &mut reads).await {
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
