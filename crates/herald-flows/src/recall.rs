//! Each living post of a report a route was sent an advisories section in,
//! kept in the report's cursor before the section is sent, and edited to
//! one fixed line once the route no longer receives the section: removed
//! from the config, no longer living, no longer naming the report, no
//! longer private, or moved to another place (ADR-0067).

use herald_config::{Config, RECALL_TEMPLATE, RouteConfig};
use herald_core::{
    AdvisedPost, AdvisedReports, Board, Cursor, Delivery, Identity, Key, MOST_ADVISED_REPORTS,
    Route, RouteId, Showing, SourceName, Step, TemplateName, Visibility, advises, living_identity,
    report_event,
};
use herald_ports::{Clock, Cursors, Http, Ledger, Recalls, Signer, Store, Versioned, Written};

use crate::announce::{Announced, Outcome, Ports, Run, Stop, stored};
use crate::audience::Audiences;
use crate::poll::{Fresh, Remembered, Sweep, remembered};
use crate::prepare::{Connected, PreparedRoute};

/// Why a post is not edited where its destination is no longer configured.
pub(crate) const UNCONFIGURED: &str = "its destination is no longer in the config";

/// Why a post is not edited where its destination cannot place it again.
pub(crate) const UNPLACED: &str = "its destination cannot find the post from what herald kept";

/// Why a post is not edited where its destination cannot edit.
pub(crate) const UNEDITABLE: &str =
    "its destination cannot edit a post, so the section cannot be recalled";

/// Why a post is not edited where no route this run reaches its destination.
pub(crate) const UNREACHED: &str = "no route of this run reaches its destination";

/// Why a post is not edited where its ledger entry is gone.
pub(crate) const UNLEDGERED: &str = "herald no longer holds the post in its ledger";

/// Why a living post is sent the withheld line where the store keeps no
/// names of the reports whose posts were sent advisories.
pub(crate) const UNINDEXED: &str = "the store keeps no names of the reports whose living posts were sent advisories, so herald could not find the post again to edit the section out";

/// Why a living post is sent the withheld line where the names of the
/// reports whose posts were sent advisories reached their most.
pub(crate) const UNBOUNDED: &str =
    "the store already names the most reports whose living posts were sent advisories";

/// How many times the names are written again where another run wrote them
/// first, before the post is sent the withheld line.
const ATTEMPTS: usize = 3;

/// The record naming every report whose cursor may keep a living post an
/// advisories section was sent in, `name` among them: where `renewed`,
/// written with a new version whether or not it named it, so a walk that
/// read it before cannot drop the name; else written only where it does
/// not name it (ADR-0067).
///
/// # Errors
///
/// Where the store keeps no such record, could not read it, refused the
/// write, holds the most names already, or another run kept writing it
/// first: the section is then withheld.
async fn indexed<S: Recalls>(recalls: &S, name: &str, renewed: bool) -> Result<(), Stop> {
    for _ in 0..ATTEMPTS {
        let record = recalls
            .advised()
            .await
            .map_err(|_| Stop::Store(UNINDEXED.to_owned()))?;
        let mut reports = record
            .as_ref()
            .map(|record| record.value.clone())
            .unwrap_or_default();
        if !renewed && reports.reports.contains(name) {
            return Ok(());
        }
        if !reports.reports.contains(name) && reports.reports.len() >= MOST_ADVISED_REPORTS {
            return Err(Stop::Store(UNBOUNDED.to_owned()));
        }
        reports.reports.insert(name.to_owned());
        let written = recalls
            .put_advised(&reports, record.map(|record| record.version))
            .await
            .map_err(stored)?;
        if matches!(written, Written::Written(_)) {
            return Ok(());
        }
    }
    Err(Stop::Store(
        "another run kept writing the names of the reports whose living posts were sent advisories"
            .to_owned(),
    ))
}

/// Whether `post` of the report `name` still receives the section: the
/// report's view still reads advisories, and its route is in `config`,
/// keeps a living post of the report at the same place of the same
/// destination, compared as the destination's kind reads both so a new
/// option written with its default moves nothing, and is shown the section.
pub(crate) fn receives(config: &Config, name: &str, post: &AdvisedPost) -> bool {
    let advising = config
        .reports
        .get(name)
        .and_then(|report| config.views.get(&report.view))
        .is_some_and(|view| view.advisories.is_some());
    let kept = config
        .destinations
        .get(&post.destination)
        .and_then(|destination| destination.recalled(&post.placed))
        .and_then(|placed| placed.written());
    advising
        && config
            .routes
            .iter()
            .find(|route| route.route.id == post.route)
            .is_some_and(|route| {
                route.route.delivery.living
                    && route.route.destination == post.destination
                    && kept.is_some()
                    && route.options.written() == kept
                    && route
                        .route
                        .filter
                        .sources
                        .iter()
                        .any(|source| source.as_str() == name)
                    && advises(config.surface(&route.route.id))
            })
}

/// The record of the living post `route` keeps of a report.
pub(crate) fn post_of(route: &RouteConfig) -> AdvisedPost {
    AdvisedPost {
        route: route.route.id.clone(),
        destination: route.route.destination.clone(),
        placed: route.options.written().unwrap_or_default(),
    }
}

/// The living posts a section is about to be sent to, at least one: made
/// only by [`Recording::of`], so a cursor is never recorded for a send
/// that holds no post (ADR-0067).
pub(crate) struct Recording(Vec<AdvisedPost>);

impl Recording {
    /// `posts`, where it holds any.
    pub(crate) fn of(posts: Vec<AdvisedPost>) -> Option<Self> {
        (!posts.is_empty()).then_some(Self(posts))
    }
}

/// `record` of the report `name` with each of `advised` it does not hold,
/// written before the section is sent, between two writes of the names of
/// the reports whose living posts were sent advisories, so no cursor keeps
/// such a post its report is not named for once it is sent; the record as
/// it stands where it holds them all, once the names hold `name` too, as
/// a run that overlapped another may have left them without it.
///
/// # Errors
///
/// Where the store refused a write, so the section is withheld.
pub(crate) async fn recorded<S: Store, C: Clock, G: Signer>(
    ports: &Ports<'_, S, C, G>,
    name: &str,
    record: Option<Versioned<Cursor>>,
    advised: Recording,
) -> Result<Option<Versioned<Cursor>>, Stop> {
    let mut cursor = record
        .as_ref()
        .map(|record| record.value.clone())
        .unwrap_or_default();
    let recalls = ports.store.recalls();
    let fresh: Vec<AdvisedPost> = advised
        .0
        .into_iter()
        .filter(|post| !cursor.advised.contains(post))
        .collect();
    if fresh.is_empty() {
        indexed(&recalls, name, false).await?;
        return Ok(record);
    }
    cursor.advised.extend(fresh);
    indexed(&recalls, name, true).await?;
    let written = ports
        .store
        .cursors()
        .put_cursor(
            &SourceName::new(name),
            &cursor,
            record.as_ref().map(|record| record.version),
        )
        .await
        .map_err(stored)?;
    match written {
        Written::Written(version) => {
            indexed(&recalls, name, true).await?;
            Ok(Some(Versioned {
                value: cursor,
                version,
            }))
        }
        Written::Conflict => Err(Stop::Store(
            "another run wrote the report's cursor first".to_owned(),
        )),
    }
}

/// The route `post` is edited through: its destination reached again at
/// the place herald kept, through what a route of this run that reaches
/// the same destination holds; or why it cannot be.
fn recalling<'c, H>(
    (config, connected): (&Config, &'c Connected<H>),
    post: &AdvisedPost,
) -> Result<(PreparedRoute, &'c H), &'static str> {
    let destination = config
        .destinations
        .get(&post.destination)
        .ok_or(UNCONFIGURED)?;
    let placed = destination.recalled(&post.placed).ok_or(UNPLACED)?;
    let (mut profile, _) = destination.profile(&placed).ok_or(UNPLACED)?;
    if !profile.features.edits {
        return Err(UNEDITABLE);
    }
    let (borrowed, http) = connected
        .routes
        .iter()
        .find(|(route, _)| route.config.route.destination == post.destination)
        .ok_or(UNREACHED)?;
    let reached = destination
        .reach(&placed, &borrowed.base, borrowed.secret.clone())
        .ok_or(UNPLACED)?;
    profile.symbols = destination.symbols();
    let config = RouteConfig {
        route: Route {
            id: post.route.clone(),
            filter: borrowed.config.route.filter.clone(),
            destination: post.destination.clone(),
            template: Some(TemplateName::new(RECALL_TEMPLATE)),
            delivery: Delivery {
                living: true,
                ..Delivery::default()
            },
        },
        options: placed,
        profile,
        private: true,
        vouched: false,
        stated: true,
        unstated: Vec::new(),
        taken: borrowed.config.taken.clone(),
    };
    Ok((
        PreparedRoute {
            config,
            reached,
            secret: borrowed.secret.clone(),
            marker_key: borrowed.marker_key.clone(),
            derived: borrowed.derived,
            domain: borrowed.domain.clone(),
            base: borrowed.base.clone(),
            attempts: borrowed.attempts,
            signed_for: borrowed.signed_for,
        },
        http,
    ))
}

/// How many requests a run keeps for each recorded post its recall may
/// bring to the fixed line, before it sends anything else: one deletion
/// of the post itself and one post of one part, however long the section
/// was (ADR-0067).
pub(crate) const RECALL_REQUESTS: usize = 2;

/// The ledger key of the living post of the report `name` on the route
/// `post` names.
pub(crate) fn living_key(name: &str, post: &AdvisedPost) -> Key {
    Key::derive(&post.route, &Identity::new(living_identity(name)))
}

/// What became of recalling `post` of the report `name`: its living post
/// edited to the fixed line, or why it was not; and whether the post is
/// still kept, as a part of it sent and never found again may still show
/// the section.
async fn recalled_one<S: Store, C: Clock, G: Signer, H: Http>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (name, view): (&str, &str),
    post: &AdvisedPost,
    run: &str,
) -> (Outcome, bool) {
    let (route, http) = match recalling((config, connected), post) {
        Ok(recalling) => recalling,
        Err(reason) => return (Outcome::Skipped(reason.to_owned()), true),
    };
    let key = living_key(name, post);
    let posted = match ports.store.ledger().entry(&key).await {
        Ok(entry) => entry.is_some_and(|entry| {
            entry.value.replacing.is_some()
                || entry
                    .value
                    .records
                    .iter()
                    .any(|record| record.step == Step::Post)
        }),
        Err(error) => return (crate::announce::stopped(stored(error)), true),
    };
    if !posted {
        return (Outcome::Skipped(UNLEDGERED.to_owned()), true);
    }
    let board = Board::default();
    let event = match report_event((name, view), &board, ports.clock.now(), true) {
        Ok(event) => event.seen_as(Visibility::Private),
        Err(problem) => {
            return (
                crate::announce::stopped(Stop::Template(problem.to_string())),
                true,
            );
        }
    };
    let running = Run {
        ports,
        route: &route,
        kinds: &config.kinds,
        http,
        holder: run,
        lease: config.announce.lease,
        seen: Visibility::Private,
        view: Some(Showing {
            view,
            board: &board,
        }),
        advised: true,
    };
    let outcome = Box::pin(running.announce(&event, &config.templates)).await;
    let strayed = match ports.store.ledger().entry(&key).await {
        Ok(Some(entry)) => !entry.value.strays.is_empty(),
        Ok(None) | Err(_) => true,
    };
    (outcome, strayed)
}

/// Each living post the cursor of the report `name` keeps that its route no
/// longer receives the section in, or whose place this run's check of its
/// audience, kept in `audiences`, now withholds one from, brought to the
/// fixed line once a round;
/// each one edited dropped from the cursor, and each one that could not be
/// kept for the next round, never stopping the report. Whether the cursor
/// keeps any such post once it is done, with what it swept.
async fn recalled<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    name: &str,
    (run, audiences): (&str, &mut Audiences),
) -> (bool, Option<Sweep>) {
    let (config, connected, ports) = flow;
    let record = match ports.store.cursors().cursor(&SourceName::new(name)).await {
        Ok(Some(record)) => record,
        Ok(None) => return (false, None),
        Err(_) => return (true, None),
    };
    let reserve = record.value.advised.len();
    let (mut kept, mut due) = (Vec::new(), Vec::new());
    for post in record.value.advised.iter().cloned() {
        let reached = connected
            .routes
            .iter()
            .any(|(route, _)| route.config.route.id == post.route);
        let still = receives(config, name, &post)
            && (!reached
                || crate::audience::withheld(flow, (&post.route, reserve), audiences)
                    .await
                    .is_none());
        if still {
            kept.push(post);
        } else {
            due.push(post);
        }
    }
    if due.is_empty() {
        return (!kept.is_empty(), None);
    }
    let view = config
        .reports
        .get(name)
        .map_or("", |report| report.view.as_str());
    let mut left = kept;
    let mut announced = Vec::new();
    for post in due {
        let (outcome, kept) = recalled_one(flow, (name, view), &post, run).await;
        if kept || !matches!(outcome, Outcome::Sent { .. } | Outcome::AlreadyAnnounced) {
            left.push(post.clone());
        }
        announced.push(Announced {
            route: post.route,
            outcome: crate::announce::sealed(outcome),
        });
    }
    let unchanged = left.len() == record.value.advised.len();
    let holds = !left.is_empty();
    let cursor = if unchanged {
        Remembered::Kept
    } else {
        let advanced = Cursor {
            advised: left,
            ..record.value.clone()
        };
        let written = ports
            .store
            .cursors()
            .put_cursor(&SourceName::new(name), &advanced, Some(record.version))
            .await;
        remembered(written, "the cursor")
    };
    let holds = holds || matches!(cursor, Remembered::Failed(_));
    let Ok(event) = report_event((name, view), &Board::default(), ports.clock.now(), true) else {
        return (true, None);
    };
    let sweep = Sweep::Reported {
        pulls: 0,
        announced: vec![Fresh {
            event: event.seen_as(Visibility::Private),
            announced,
            held: None,
        }],
        cursor,
    };
    (holds, Some(sweep))
}

/// Every living post of every report the store names, configured or not,
/// whose route no longer receives the section or whose place's audience,
/// checked once a run for each place and kept in `audiences`, now withholds
/// one, brought to the fixed line once a run before any section is sent:
/// deleted and posted anew where its destination keeps what an edit
/// replaced; each report whose cursor no longer keeps such a post dropped
/// from the names, with compare-and-set so a run recording one meanwhile
/// keeps it named (ADR-0067).
pub(crate) async fn recalled_all<S: Store, C: Clock, G: Signer, H: Http>(
    flow: (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (run, audiences): (&str, &mut Audiences),
) -> Vec<(String, Sweep)> {
    let recalls = flow.2.store.recalls();
    let Ok(Some(record)) = recalls.advised().await else {
        return Vec::new();
    };
    let mut swept = Vec::new();
    let mut named = AdvisedReports::default();
    for name in &record.value.reports {
        let (holds, sweep) = Box::pin(recalled(flow, name, (run, &mut *audiences))).await;
        if holds {
            named.reports.insert(name.clone());
        }
        swept.extend(sweep.map(|sweep| (name.clone(), sweep)));
    }
    if named != record.value {
        drop(recalls.put_advised(&named, Some(record.version)).await);
    }
    swept
}

/// Each post the cursor of every report the store names records, by its
/// route, and whether its route still receives the section: what a run
/// keeps requests for before it sends anything else (ADR-0067).
pub(crate) async fn recorded_posts<S: Store>(config: &Config, store: &S) -> Vec<(RouteId, bool)> {
    let Ok(Some(named)) = store.recalls().advised().await else {
        return Vec::new();
    };
    let mut posts = Vec::new();
    for name in &named.value.reports {
        if let Ok(Some(record)) = store.cursors().cursor(&SourceName::new(name)).await {
            posts.extend(
                record
                    .value
                    .advised
                    .iter()
                    .map(|post| (post.route.clone(), receives(config, name, post))),
            );
        }
    }
    posts
}
