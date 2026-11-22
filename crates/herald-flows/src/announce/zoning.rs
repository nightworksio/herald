//! The areas the pull request an event is of falls in, for the routes
//! narrowing by areas: from the pull request as herald keeps it, or from
//! the forge as the event is delivered (ADR-0072).

use herald_config::Config;
use std::sync::Arc;

use herald_core::{
    AreaPaths, Attribute, BASE, COMMIT, Changed, Event, NUMBER, Origin, PULL_LINK, PullKey,
    RepositoryKey, SourceName,
};
use herald_kinds::{Queue, Source};
use herald_ports::{Clock, DynHttp, Http, Pulls, Store};

use super::{Ports, Within};
use crate::budget::Budgeted;
use crate::codeowned::{Codeowned, Reach, names};
use crate::prepare::{Connected, PreparedRoute};

/// The event as each route narrowing by areas sees it: with the areas its
/// pull request's kept paths fall in, and with those its delivered paths
/// fall in, where a route reading each takes it at all.
#[derive(Debug, Default)]
pub(super) struct Zoned {
    kept: Option<Event>,
    delivered: Option<Event>,
}

impl Zoned {
    /// Each of `reachable` that takes `event` of `source`, through the
    /// owner's member `within` names where it names one, with the event as
    /// it sees it.
    pub(super) fn taking<'c, 'e, H>(
        &'e self,
        reachable: Vec<&'c (PreparedRoute, H)>,
        within: Within<'_>,
        (source, event): (&SourceName, &'e Event),
    ) -> Vec<(&'c (PreparedRoute, H), &'e Event)> {
        reachable
            .into_iter()
            .map(|reached| (reached, self.seen_by(&reached.0, event)))
            .filter(|((route, _), seen)| {
                let route = &route.config.route;
                within.reach.map_or_else(
                    || route.filter.takes(source, seen),
                    |reach| reach.takes(route, source, seen),
                )
            })
            .collect()
    }

    /// The event as `route` sees it: `event` itself where no areas were
    /// read for it.
    fn seen_by<'e>(&'e self, route: &PreparedRoute, event: &'e Event) -> &'e Event {
        match route.config.route.filter.area_paths {
            AreaPaths::Kept => self.kept.as_ref(),
            AreaPaths::Delivery => self.delivered.as_ref(),
        }
        .unwrap_or(event)
    }
}

/// The pull request `event` is of, where it is one's and its origin names
/// no areas yet: its key, its repository's full path and its head commit.
fn pulled(event: &Event) -> Option<(PullKey, &Origin, Option<&str>)> {
    let origin = event.origin().filter(|origin| origin.areas.is_none())?;
    let Some(Attribute::Number(number)) = event.attribute(NUMBER) else {
        return None;
    };
    let repository = RepositoryKey::of(event.link(PULL_LINK)?, &origin.id)?;
    let commit = match event.attribute(COMMIT) {
        Some(Attribute::Text(commit)) => Some(commit.as_str()),
        _ => None,
    };
    Some((
        PullKey {
            repository,
            number: number.as_u64()?,
        },
        origin,
        commit,
    ))
}

/// The names of the areas `source` names the paths `changed` lists fall
/// in, in the repository at `repository` merging into `base`, reading its
/// CODEOWNERS file through `reach` where it reads areas from one, where
/// `changed` lists every path; none where it does not, or the file could
/// not be read.
async fn fallen<P: Pulls>(
    codeowned: &mut Codeowned<'_, P>,
    (source, reach): (&Source, Option<Reach<'_>>),
    (repository, base, changed): (&str, Option<&str>, &Changed),
) -> Option<Vec<String>> {
    if !changed.whole {
        return None;
    }
    codeowned
        .areas((source, reach), (repository, base, Some(changed)))
        .await
        .ok()
        .map(names)
}

/// `event` with its origin naming `areas`.
fn with_areas(event: &Event, origin: &Origin, areas: Vec<String>) -> Event {
    let mut origin = origin.clone();
    origin.areas = Some(areas);
    event.clone().from_origin(origin)
}

/// `event`, from `source`, as each of `routes`, those it may reach, sees
/// it where one narrows by areas: the areas of the pull request it is of,
/// by its source's areas for its repository, from the paths kept for the
/// event's head commit, and from one read of the forge's where a route
/// reads them at delivery; none where they are not known in full.
pub(super) async fn zoned<S: Store, C: Clock, G, H: Http>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (source, event): (&SourceName, &Event),
    routes: &[&(PreparedRoute, H)],
) -> Zoned {
    let reading = |paths| {
        routes.iter().any(|(route, _)| {
            let filter = &route.config.route.filter;
            filter.area_paths == paths && filter.matching.reads_areas()
        })
    };
    let (kept, delivered) = (reading(AreaPaths::Kept), reading(AreaPaths::Delivery));
    let Some((key, origin, commit)) = pulled(event).filter(|_| kept || delivered) else {
        return Zoned::default();
    };
    let Some(named) = config
        .sources
        .get(source)
        .filter(|named| named.areas(&origin.repository).is_some())
    else {
        return Zoned::default();
    };
    let queued = queued(connected, (source, &origin.repository));
    let http = queued.map(|(_, http)| Budgeted::new(http, ports.budget, ports.clock));
    let reach: Option<Reach<'_>> = queued
        .zip(http.as_ref())
        .map(|((queue, _), http)| (queue.as_ref(), http as &dyn DynHttp));
    let mut codeowned = Codeowned::new((ports.store.pulls(), source, ports.clock.now()));
    let held = match (kept, commit, ports.store.pulls()) {
        (true, Some(commit), Some(pulls)) => match pulls.pull(&key).await {
            Ok(Some(held)) => {
                Some(held.value.pull).filter(|pull| pull.head.as_deref() == Some(commit))
            }
            _ => None,
        },
        _ => None,
    };
    let kept = match held
        .as_ref()
        .and_then(|pull| Some((pull.files.as_ref()?, pull.base.as_deref())))
    {
        Some((changed, base)) => {
            fallen(
                &mut codeowned,
                (named, reach),
                (&origin.repository, base, changed),
            )
            .await
        }
        None => None,
    };
    let delivered = match reach.filter(|_| delivered) {
        Some((queue, http)) => match queue.changed(http, (&origin.repository, key.number)).await {
            Ok(changed) => {
                let base = match event.attribute(BASE) {
                    Some(Attribute::Text(base)) => Some(base.as_str()),
                    _ => None,
                };
                fallen(
                    &mut codeowned,
                    (named, reach),
                    (&origin.repository, base, &changed),
                )
                .await
            }
            Err(_) => None,
        },
        None => None,
    };
    Zoned {
        kept: kept.map(|areas| with_areas(event, origin, areas)),
        delivered: delivered.map(|areas| with_areas(event, origin, areas)),
    }
}

/// The queue of `source` that reads the repository at `repository`, with
/// the HTTP it is asked through.
fn queued<'c, H>(
    connected: &'c Connected<H>,
    (source, repository): (&SourceName, &str),
) -> Option<(&'c Arc<dyn Queue>, &'c H)> {
    connected.sources.iter().find_map(|(prepared, http)| {
        prepared
            .queue
            .as_ref()
            .filter(|queue| prepared.name == *source && queue.holds(repository))
            .map(|queue| (queue, http))
    })
}
