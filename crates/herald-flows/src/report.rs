//! Each report whose window began and was not yet announced: its view's
//! open pull requests read from every source it names, and announced
//! through the routes naming the report (ADR-0055).

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use herald_config::{Config, Report, ViewConfig};
use herald_core::{
    Advised, Advisories, Beside, Board, Cursor, Given, Identity, Listed, Loads, Overviews, Printed,
    Pull, RouteId, Row, Showing, SourceName, Surface, Teams, Visibility, advises, grouped, joined,
    report_event, steps, teams_reach, teams_reach_each,
};
use herald_kinds::{Asked, Queue, Scope, Teamed};
use herald_ports::{Clock, Cursors, Http, Signer, Store, Versioned};
use jiff::Timestamp;

use crate::announce::{Outcome, Ports, Stop, Within, announce_within};
use crate::budget::Budgeted;
use crate::codeowned::Codeowned;
use crate::deciding::decided_rows;
use crate::halted::Halted;
use crate::poll::{Fresh, Remembered, Sweep, Swept, remembered, stored};
use crate::prepare::Connected;
use crate::recall::Recording;

mod capacity;

/// Why a report's view could not be read: a source it names is not one
/// this run reads.
const UNREAD_SOURCE: &str = "the view reads a source this run does not poll";

/// What each view a round read, by the view's name, so a report and a rule
/// watching one view read it once a round.
pub(crate) type Reads = BTreeMap<String, Read>;

/// What one read of a view found: its open pull requests, its overviews
/// (ADR-0066), the reviews kept on them (ADR-0075), its sources' forge
/// teams where a surface it reaches may count them, held for the round
/// alone (ADR-0077), and its advisories
/// where a consumer could be shown them, read after the rows and reaching
/// a board only through
/// [`crate::advised::shown`] for the surface it leaves through (ADR-0067).
#[derive(Debug, Clone, Default)]
pub(crate) struct Read {
    pub(crate) pulls: Vec<Pull>,
    pub(crate) overviews: Overviews,
    pub(crate) advisories: Option<Advisories>,
    pub(crate) given: Given,
    pub(crate) teams: Teams,
}

impl Read {
    /// Who may see the strictest of what it holds.
    fn strictest(&self) -> Visibility {
        let runs = self
            .overviews
            .capacity
            .iter()
            .flatten()
            .map(|run| run.visibility);
        let unread = self.overviews.unread.iter().map(|unread| unread.visibility);
        let header = self
            .overviews
            .header
            .as_ref()
            .and_then(herald_core::Header::strictest);
        self.pulls
            .iter()
            .map(|pull| pull.visibility)
            .chain(runs)
            .chain(unread)
            .chain(header)
            .max()
            .unwrap_or(Visibility::Public)
    }
}

/// Each source a view reads, as this run reaches it: its name, its queue
/// and the `Http` that reaches it. Made only by [`sources`], so a view's
/// advisories are read from no source its rows were not read from.
pub(crate) struct Sources<'c, H>(Vec<(&'c SourceName, &'c Arc<dyn Queue>, &'c H)>);

impl<'c, H> Sources<'c, H> {
    /// Each source, in the order the view names them.
    pub(crate) fn iter(
        &self,
    ) -> impl Iterator<Item = &(&'c SourceName, &'c Arc<dyn Queue>, &'c H)> {
        self.0.iter()
    }
}

/// Each source `view` reads, as `connected` reaches it: the one place a
/// view's sources are found, so its advisories are read only from the
/// sources its rows are.
///
/// # Errors
///
/// Where the view reads a source this run does not reach.
pub(crate) fn sources<'c, H>(
    connected: &'c Connected<H>,
    view: &ViewConfig,
) -> Result<Sources<'c, H>, Stop> {
    view.sources
        .iter()
        .map(|name| {
            connected
                .sources
                .iter()
                .find(|(source, _)| source.name == *name)
                .and_then(|(source, http)| {
                    source
                        .queue
                        .as_ref()
                        .map(|queue| (&source.name, queue, http))
                })
                .ok_or_else(|| Stop::Source(UNREAD_SOURCE.to_owned()))
        })
        .collect::<Result<_, _>>()
        .map(Sources)
}

/// The open pull requests and overviews of the view `name`, read once a
/// round into `reads`, and its advisories where `advised` says a consumer
/// can be shown them, read once a round after its rows and leaving as many
/// requests as `advised` names for the posts.
pub(crate) async fn view_pulls<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (name, view, advised): (&str, &ViewConfig, Option<usize>),
    reads: &mut Reads,
) -> Result<Read, Stop> {
    let sources = sources(flow.1, view)?;
    let config = flow.0;
    let teams = config
        .routes
        .iter()
        .any(|route| teams_reach(config.surface(&route.route.id)));
    let mut read = match reads.get(name) {
        Some(read) => read.clone(),
        None => read_pulls((config, &sources, flow.2), (view, teams))
            .await
            .map_err(|halted| halted.stop)?,
    };
    if let (Some(kept), Some(advising), None) = (advised, view.advisories, &read.advisories) {
        let found =
            crate::advised::read((&sources, flow.2), (view, advising, kept), &read.pulls).await;
        read.advisories = Some(found);
    }
    reads.insert(name.to_owned(), read.clone());
    Ok(read)
}

/// The open pull requests of every source `view` reads, live or kept as
/// the view and the store say (ADR-0061), the overviews it shows
/// (ADR-0066), and, where `teams` says a surface it reaches may count them
/// and it shows its load, the forge teams of each source reading them, an
/// owner's once (ADR-0077); the first source that could not be read, and
/// the run's budget, stopping it, named with its stop.
pub(crate) async fn read_pulls<S: Store, C: Clock, G: Signer, H: Http>(
    (config, reached, ports): (&Config, &Sources<'_, H>, &Ports<'_, S, C, G>),
    (view, teams): (&ViewConfig, bool),
) -> Result<Read, Halted> {
    let mut pulls = Vec::new();
    let mut given = Given::default();
    let mut overviews = Overviews {
        capacity: view.capacity.map(|_| Vec::new()),
        ..Overviews::default()
    };
    let (mut headed, mut checked) = (0, 0);
    for &(name, queue, http) in reached.iter() {
        let members = scoped_members(ports, (name, queue.as_ref())).await?;
        let source = config.sources.get(name);
        let zoned =
            source.is_some_and(|source| !source.area_names().is_empty() || source.codeowned());
        let asked = Asked {
            most: view.most,
            scope: members.as_ref().map(|(owner, members)| Scope {
                owner: *owner,
                members,
                narrowed: false,
            }),
            stated: view.stated,
            files: view.conflicts || zoned,
            queue: view.queue,
        };
        let keeps = ports.store.pulls().is_some();
        let http = &Budgeted::new(http, ports.budget, ports.clock);
        let mut read = if crate::kept::reads_kept((config, keeps), view, name) {
            let detailed = config
                .sources
                .get(name)
                .and_then(|source| source.queue_cost(view.most))
                .is_some_and(|cost| cost.detailed);
            let (read, kept) = crate::kept::kept_pulls(
                ports,
                (name, queue.as_ref(), http),
                (view, asked, detailed),
            )
            .await
            .map_err(|stop| Halted::of(name, stop))?;
            given.merge(kept);
            read
        } else {
            given.live = true;
            queue
                .opened(http, asked)
                .await
                .map_err(|unpolled| Halted::of(name, http.halted(unpolled)))?
        };
        if let Some(source) = source {
            let keeping = (ports.store.pulls(), ports.clock.now());
            Codeowned::new((name, source, Some((queue.as_ref(), http))), keeping)
                .zone(&mut read)
                .await
                .map_err(|unpolled| Halted::of(name, http.halted(unpolled)))?;
        }
        if let Some(capacity) = view.capacity {
            let reached = (name, queue.as_ref(), http);
            let asking = (asked, members.as_ref());
            let counted =
                capacity::capacity_of((view, capacity), reached, asking, &read, ports).await;
            overviews.merge(counted.within(&view.view, |path: &str| queue.holds(path)));
        }
        let at = (read.as_slice(), ports.clock.now());
        if let Some(header) =
            crate::header::header(&view.view, (queue.as_ref(), http), at, &mut headed).await
        {
            overviews.header.get_or_insert_default().merge(header);
        }
        let reached = (name, queue.as_ref(), http);
        let keeping = (ports.store.pulls(), ports.clock.now());
        let bases = crate::based::bases(&view.view, reached, (&read, keeping), &mut checked).await;
        overviews.bases.extend(bases);
        pulls.extend(read);
    }
    let forge = if teams && view.load.is_some() {
        forge_teams((config, reached, ports)).await?
    } else {
        Teams::new()
    };
    overviews.heads = Overviews::heads_of(&pulls);
    Ok(Read {
        pulls,
        overviews,
        advisories: None,
        given,
        teams: forge,
    })
}

/// The members of the owner the source `name` reads through `queue`, where
/// it reads an owner's repositories.
async fn scoped_members<S: Store, C: Clock, G: Signer>(
    ports: &Ports<'_, S, C, G>,
    (name, queue): (&SourceName, &dyn Queue),
) -> Result<Option<(herald_core::Owner, Vec<Listed>)>, Halted> {
    if !queue.scoped() {
        return Ok(None);
    }
    crate::scope::members(ports.store, name)
        .await
        .map(Some)
        .map_err(|stop| Halted::of(name, stop))
}

/// The forge teams of each of `reached` that reads them, an owner's once,
/// each read through the run's budget; the first that could not be read
/// stopping the read, named with its stop, and one whose forge refuses
/// left out (ADR-0077).
async fn forge_teams<S: Store, C: Clock, G: Signer, H: Http>(
    (config, reached, ports): (&Config, &Sources<'_, H>, &Ports<'_, S, C, G>),
) -> Result<Teams, Halted> {
    let mut forge = Teams::new();
    let mut owners = std::collections::BTreeSet::new();
    for &(name, queue, http) in reached.iter() {
        let reads = config
            .sources
            .get(name)
            .is_some_and(|source| source.forge_teams());
        if reads && owners.insert(queue.team_owner()) {
            let http = &Budgeted::new(http, ports.budget, ports.clock);
            let read = queue
                .teams(http)
                .await
                .map_err(|unpolled| Halted::of(name, http.halted(unpolled)))?;
            if let Teamed::Read(read) = read {
                forge = joined(&forge, &read);
            }
        }
    }
    Ok(forge)
}

/// The board of the view `name`, its sources' open pull requests read now
/// with each installation's token minted first, as `herald queue` shows
/// it (ADR-0055): its advisories where `private` says the operator's output
/// is private, and else withheld (ADR-0067).
///
/// # Errors
///
/// Where the config has no such view, or a source it reads could not be
/// read.
pub async fn queue<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    (name, printed): (&str, Printed),
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
    let sources = sources(connected, view)?;
    let surface = Surface::Queue { private: printed };
    let read = read_pulls((config, &sources, ports), (view, teams_reach(surface)))
        .await
        .map_err(|halted| halted.said(printed))?;
    let (pulls, withheld) = printed.printed(read.pulls);
    let unprinted = herald_core::rows(&view.view, &withheld, ports.clock.now());
    let found = match view.advisories {
        Some(advising) if advises(surface) => {
            Some(crate::advised::read((&sources, ports), (view, advising, 0), &pulls).await)
        }
        _ => None,
    };
    let decided = decided_rows(flow, view, &pulls).await;
    let teams = joined(&config.teams, &read.teams);
    let load = view
        .load
        .map(|load| Loads::of(&decided, &read.given, (&teams, load), ports.clock.now()));
    let beside = Beside {
        withheld: &unprinted,
        bases: &read.overviews.bases,
        sees: printed.sees(),
    };
    let mut board = grouped(&view.view, decided, beside);
    board.overviews = printed.overviews(read.overviews, &pulls);
    board.overviews.load = load;
    board.overviews.advisories = view
        .advisories
        .map(|_| crate::advised::shown(found.as_ref(), surface));
    board.withheld = withheld.len();
    Ok(board)
}

/// The rows of `part`'s view withheld from it: those of every row it is
/// not shown.
fn withheld(part: &Part<'_>) -> Vec<Row> {
    let shown: HashSet<&str> = part.rows.iter().map(|row| row.pull.url.as_str()).collect();
    part.every
        .iter()
        .filter(|row| !shown.contains(row.pull.url.as_str()))
        .cloned()
        .collect()
}

/// What one announcement of a report shows: the pull requests, every row
/// of its view, the reviews kept on them, the forge teams read, who may
/// see them, the routes it may reach, where only some, whether those keep
/// one living post of it, and those of them that are or were sent its
/// view's advisories section.
struct Part<'a> {
    view: &'a ViewConfig,
    overviews: &'a Overviews,
    given: &'a Given,
    teams: &'a Teams,
    advisories: Option<Advised>,
    rows: &'a [Row],
    every: &'a [Row],
    visibility: Visibility,
    routes: &'a [RouteId],
    living: bool,
    advised: &'a [RouteId],
}

/// `report`'s window that began at `start`, showing `part`, announced to
/// the routes naming the report among those `part` may reach.
async fn announced_part<S: Store, C: Clock, G: Signer, H: Http>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (name, report, start): (&str, &Report, Timestamp),
    part: Part<'_>,
    run: &str,
) -> Result<Fresh, Stop> {
    let unshown = withheld(&part);
    let beside = Beside {
        withheld: &unshown,
        bases: &part.overviews.bases,
        sees: part.visibility,
    };
    let mut shown = grouped(&part.view.view, part.rows.to_vec(), beside);
    shown.overviews = part.overviews.seen_by(part.visibility);
    shown.overviews.advisories = part.advisories;
    let teamed = teams_reach_each(part.routes.iter().map(|id| config.surface(id)));
    let unread = Teams::new();
    let teams = joined(&config.teams, if teamed { part.teams } else { &unread });
    shown.overviews.load = part
        .view
        .load
        .map(|load| Loads::of(part.rows, part.given, (&teams, load), ports.clock.now()));
    let event = report_event((name, &report.view), &shown, start, part.living)
        .map_err(|problem| Stop::Source(problem.to_string()))?
        .seen_as(part.visibility);
    let within = Within {
        routes: Some(part.routes),
        view: Some(Showing {
            view: &report.view,
            board: &shown,
        }),
        advised: part.advised,
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
/// rule `name` has not yet remembered it, or remembers routes of it still
/// pending: its identity, and the cursor as the store keeps it.
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
    let seen = record.as_ref().is_some_and(|record| {
        record.value.seen.first() == Some(&identity) && record.value.pending.is_empty()
    });
    Ok((!seen).then_some((identity, record)))
}

/// What became of the cursor of the report or rule `name` once `announced`
/// went out: kept where a route no advisories section reaches stopped;
/// else the window `identity` remembered over `record`, each route sent a
/// section that stopped kept pending, so the next run sends it alone and
/// no other route's post depends on how long the section was (ADR-0067).
pub(crate) async fn settled<S: Store, C: Clock, G: Signer>(
    ports: &Ports<'_, S, C, G>,
    (name, identity): (&str, Identity),
    record: Option<&Versioned<Cursor>>,
    announced: &[Fresh],
) -> Remembered {
    let routes = || announced.iter().flat_map(|fresh| &fresh.announced);
    if routes().any(|announced| matches!(announced.outcome, Outcome::Stopped { .. })) {
        return Remembered::Kept;
    }
    let pending = routes()
        .filter(|announced| matches!(announced.outcome, Outcome::Sealed { stopped: true }))
        .map(|announced| announced.route.clone())
        .collect();
    let advanced = Cursor {
        seen: vec![identity],
        pending,
        advised: record
            .map(|record| record.value.advised.clone())
            .unwrap_or_default(),
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

/// The routes of `connected` that keep a living post of a report.
fn living<H>(connected: &Connected<H>) -> Vec<RouteId> {
    connected
        .routes
        .iter()
        .filter(|(route, _)| route.config.living())
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

/// `routes`, none left empty, parted where `teams`, a round's forge teams,
/// holds any into those a forge team may reach and the rest, so a private
/// route is shown them and no other route is (ADR-0077).
fn reaching(config: &Config, routes: Vec<RouteId>, teams: &Teams) -> Vec<Vec<RouteId>> {
    let parts = if teams.is_empty() {
        vec![routes]
    } else {
        let (reached, rest): (Vec<RouteId>, Vec<RouteId>) = routes
            .into_iter()
            .partition(|id| teams_reach(config.surface(id)));
        vec![reached, rest]
    };
    parts.into_iter().filter(|part| !part.is_empty()).collect()
}

/// One announcement of a report still to be made: the rows it shows, who
/// may see them, its routes, whether they keep a living post, and the
/// surface they make where its view shows advisories.
type Parted = (Vec<Row>, Visibility, Vec<RouteId>, bool, Option<Surface>);

/// A report whose window began, its view's rows read and no advisory: the
/// parts no section reaches, sent before any advisory is read or any
/// section sent, but for the routes its cursor records a section sent to,
/// which wait for the recall; and the parts that can be shown one, which
/// wait until every report's other routes have theirs (ADR-0067).
struct Begun<'r> {
    name: &'r str,
    report: &'r Report,
    view: &'r ViewConfig,
    start: Timestamp,
    identity: Identity,
    recorded: Vec<RouteId>,
    read: Read,
    decided: Vec<Row>,
    plain: Vec<Parted>,
    advised: Vec<Parted>,
    announced: Vec<Fresh>,
}

impl Begun<'_> {
    /// How many requests the posts still waiting for the section take, as
    /// many as each route's steps for a post of one part, so the reserve
    /// is the same however long the section is.
    fn waiting(&self, config: &Config) -> usize {
        self.advised
            .iter()
            .flat_map(|(_, _, routes, _, _)| routes)
            .filter_map(|id| config.routes.iter().find(|route| route.route.id == *id))
            .map(|route| steps(&route.route.delivery, &route.profile.features, 1, false).len())
            .sum()
    }

    /// Its parts no section reaches sent now to each route its cursor
    /// records no section sent to, with no advisory read; the rest kept
    /// for after the recall.
    async fn plainly<S: Store, C: Clock, G: Signer, H: Http>(
        &mut self,
        flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
        run: &str,
    ) -> Result<(), Stop> {
        for (rows, visibility, routes, living, surface) in std::mem::take(&mut self.plain) {
            let (later, now): (Vec<RouteId>, Vec<RouteId>) = routes
                .into_iter()
                .partition(|id| self.recorded.contains(id));
            if !later.is_empty() {
                self.plain
                    .push((rows.clone(), visibility, later, living, surface));
            }
            if !now.is_empty() {
                let part = (rows, visibility, now, living, surface);
                let fresh = sent(flow, (self, &[]), (part, None), run).await?;
                self.announced.push(fresh);
            }
        }
        Ok(())
    }
}

/// `part` of the report `begun` shows, shown `advisories` or the withheld
/// line where its surface is told of the section, announced to its routes,
/// those of `advised` sealed.
async fn sent<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (begun, advised): (&Begun<'_>, &[RouteId]),
    ((shown, visibility, routes, living, surface), advisories): (Parted, Option<&Advisories>),
    run: &str,
) -> Result<Fresh, Stop> {
    let part = Part {
        view: begun.view,
        overviews: &begun.read.overviews,
        given: &begun.read.given,
        teams: &begun.read.teams,
        advisories: surface.map(|surface| crate::advised::shown(advisories, surface)),
        rows: &shown,
        every: &begun.decided,
        visibility,
        routes: &routes,
        living,
        advised,
    };
    announced_part(flow, (begun.name, begun.report, begun.start), part, run).await
}

/// The report `name`, where its window began and was not yet announced,
/// its view's rows read and no advisory: where `[doctor] private_to_public`
/// refuses and it read a repository not everyone may see, its private
/// routes are shown every row and the rest only the public repositories'
/// rows, and nothing where it read no public repository's; a living route
/// is shown it as its one post.
async fn begun<'r, S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&'r Config, &Connected<H>, &Ports<'_, S, C, G>),
    (name, report): (&'r str, &'r Report),
    reads: &mut Reads,
) -> Option<Result<Begun<'r>, Sweep>> {
    let (config, connected, _) = flow;
    let start = report.schedule.window(flow.2.clock.now())?;
    let (identity, record) = match due(flow.2, name, start).await {
        Ok(due) => due?,
        Err(stop) => return Some(Err(Sweep::Stopped(stop))),
    };
    let view = config.views.get(&report.view)?;
    let read = match view_pulls(flow, (&report.view, view, None), reads).await {
        Ok(read) => read,
        Err(stop) => return Some(Err(Sweep::Stopped(stop))),
    };
    let strictest = read.strictest();
    let private = crate::poll::bounded(config, strictest);
    let decided = decided_rows(flow, view, &read.pulls).await;
    let pending = record
        .as_ref()
        .filter(|record| record.value.seen.first() == Some(&identity))
        .map(|record| record.value.pending.clone());
    let mut parts: Vec<Parted> = Vec::new();
    for (shown, visibility, routes, living) in audiences(
        (config, name),
        &decided,
        (strictest, private),
        &living(connected),
    ) {
        let routes: Vec<RouteId> = routes
            .into_iter()
            .filter(|id| pending.as_ref().is_none_or(|pending| pending.contains(id)))
            .collect();
        for routes in reaching(config, routes, &read.teams) {
            match view.advisories {
                Some(_) => parts.extend(crate::advised::parted(config, routes).into_iter().map(
                    |(surface, routes)| (shown.clone(), visibility, routes, living, Some(surface)),
                )),
                None => parts.push((shown.clone(), visibility, routes, living, None)),
            }
        }
    }
    let (advised, plain): (Vec<Parted>, Vec<Parted>) = parts
        .into_iter()
        .partition(|(_, _, _, _, surface)| surface.is_some_and(advises));
    let recorded = record
        .iter()
        .flat_map(|record| &record.value.advised)
        .map(|post| post.route.clone())
        .collect();
    Some(Ok(Begun {
        name,
        report,
        view,
        start,
        identity,
        recorded,
        read,
        decided,
        plain,
        advised,
        announced: Vec::new(),
    }))
}

/// `begun`'s parts that can be shown the section, each route whose place
/// the run finds publicly readable right now, or cannot check, moved to its
/// parts no section reaches, every check leaving `kept` of the run's
/// requests for the posts still waiting (ADR-0067).
async fn checked<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (begun, kept): (&mut Begun<'_>, usize),
    audiences: &mut crate::audience::Audiences,
) {
    for (rows, visibility, routes, living, surface) in std::mem::take(&mut begun.advised) {
        let (mut shown, mut withheld) = (Vec::new(), Vec::new());
        for id in routes {
            match crate::audience::withheld(flow, (&id, kept), audiences).await {
                None => shown.push(id),
                Some(_) => withheld.push(id),
            }
        }
        if !withheld.is_empty() {
            let unshown = Some(Surface::Route {
                private: false,
                recallable: living,
            });
            begun
                .plain
                .push((rows.clone(), visibility, withheld, living, unshown));
        }
        if !shown.is_empty() {
            begun
                .advised
                .push((rows, visibility, shown, living, surface));
        }
    }
}

/// `begun`'s routes its cursor records a section sent to, once the recall
/// ran, sent their parts as sealed; then those that can be shown its view's
/// advisories, each recorded in its cursor first, sent them once they are
/// read, leaving `kept` of the run's requests for the posts of every report
/// still waiting; then its cursor settled.
async fn finished<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    mut begun: Begun<'_>,
    (run, kept): (&str, usize),
    reads: &mut Reads,
) -> Sweep {
    let (config, _, ports) = flow;
    let name = begun.name;
    let mut record = match ports.store.cursors().cursor(&SourceName::new(name)).await {
        Ok(record) => record,
        Err(error) => return Sweep::Stopped(stored(error)),
    };
    let recorded: Vec<RouteId> = record
        .iter()
        .flat_map(|record| &record.value.advised)
        .map(|post| post.route.clone())
        .collect();
    for part in std::mem::take(&mut begun.plain) {
        match sent(flow, (&begun, &recorded), (part, None), run).await {
            Ok(fresh) => begun.announced.push(fresh),
            Err(stop) => return Sweep::Stopped(stop),
        }
    }
    let recording = Recording::of(
        begun
            .advised
            .iter()
            .flat_map(|(_, _, routes, _, _)| routes)
            .filter_map(|id| config.routes.iter().find(|route| route.route.id == *id))
            .map(crate::recall::post_of)
            .collect(),
    );
    if let Some(recording) = recording {
        let read = view_pulls(flow, (&begun.report.view, begun.view, Some(kept)), reads).await;
        if let Ok(read) = read {
            begun.read.advisories = read.advisories;
        }
        let recorded = match crate::recall::recorded(ports, name, record.clone(), recording).await {
            Ok(written) => {
                record = written;
                true
            }
            Err(_) => false,
        };
        let found = begun.read.advisories.clone().filter(|_| recorded);
        for part in std::mem::take(&mut begun.advised) {
            let advised = part.2.clone();
            match sent(flow, (&begun, &advised), (part, found.as_ref()), run).await {
                Ok(fresh) => begun.announced.push(fresh),
                Err(stop) => return Sweep::Stopped(stop),
            }
        }
    }
    let cursor = settled(
        ports,
        (name, begun.identity),
        record.as_ref(),
        &begun.announced,
    )
    .await;
    Sweep::Reported {
        pulls: begun.read.pulls.len(),
        announced: begun.announced,
        cursor,
    }
}

/// Every report of `config` whose window began and was not yet announced,
/// then every rule's nudges, each view read once. No advisory is read and
/// no section sent until every report's routes are checked and every part
/// no section reaches is sent, so neither what those posts show nor whether
/// the run's budget lets them out depends on the advisories; then the
/// recall, and the sections. Until the recall, the run keeps
/// [`crate::recall::RECALL_REQUESTS`] of its requests for each recorded
/// post it may recall, the same however long its section was (ADR-0067).
pub(crate) async fn reported<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    run: &str,
) -> Vec<Swept> {
    let mut swept = Vec::new();
    let mut reads = Reads::new();
    let named = |name: &str, outcome| Swept {
        source: SourceName::new(name),
        member: None,
        outcome,
    };
    let recorded = crate::recall::recorded_posts(flow.0, flow.2.store).await;
    let keeping = |posts: usize| flow.2.keeping(posts * crate::recall::RECALL_REQUESTS);
    let every = keeping(recorded.len());
    let mut started = Vec::new();
    for (name, report) in &flow.0.reports {
        match begun((flow.0, flow.1, &every), (name, report), &mut reads).await {
            Some(Ok(begun)) => started.push(begun),
            Some(Err(outcome)) => swept.push(named(name, outcome)),
            None => {}
        }
    }
    let kept: usize = started.iter().map(|begun| begun.waiting(flow.0)).sum();
    let mut audiences = crate::audience::Audiences::new();
    for begun in &mut started {
        checked((flow.0, flow.1, &every), (begun, kept), &mut audiences).await;
    }
    let shown: Vec<&RouteId> = started
        .iter()
        .flat_map(|begun| &begun.advised)
        .flat_map(|(_, _, routes, _, _)| routes)
        .collect();
    let due = recorded
        .iter()
        .filter(|(route, receives)| !receives || !shown.contains(&route))
        .count();
    let before = keeping(due);
    let mut waiting = Vec::new();
    for mut begun in started {
        match begun.plainly((flow.0, flow.1, &before), run).await {
            Ok(()) => waiting.push(begun),
            Err(stop) => swept.push(named(begun.name, Sweep::Stopped(stop))),
        }
    }
    for (name, rule) in &flow.0.rules {
        let rules = (flow.0, flow.1, &before);
        if let Some(outcome) = crate::rules::nudged(rules, (name, rule), run, &mut reads).await {
            swept.push(named(name, outcome));
        }
    }
    let recalled = crate::recall::recalled_all(flow, (run, &mut audiences));
    for (name, outcome) in Box::pin(recalled).await {
        swept.push(named(&name, outcome));
    }
    let mut kept: usize = waiting.iter().map(|begun| begun.waiting(flow.0)).sum();
    for begun in waiting {
        let (name, own) = (begun.name, begun.waiting(flow.0));
        let outcome = finished(flow, begun, (run, kept), &mut reads).await;
        kept = kept.saturating_sub(own);
        swept.push(named(name, outcome));
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
