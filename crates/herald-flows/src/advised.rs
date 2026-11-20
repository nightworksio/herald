//! A view's advisories as a report's routes are shown them: read only
//! where some route can be shown them, after the rows, and each route given
//! the section or the withheld line by the one rule, the surface its config
//! makes it (ADR-0067).

use herald_config::{Config, ViewConfig};
use herald_core::{
    Advised, Advising, Advisories, Listed, Pull, RouteId, Surface, Unread, Visibility, advises,
};
use herald_kinds::{Asked, Scope};
use herald_ports::{Clock, Http, Signer, Store};

use crate::announce::Ports;
use crate::budget::Budgeted;
use crate::prepare::Connected;

/// Why a source's advisories were not read where this run does not poll it.
const UNPOLLED: &str = "this run does not poll the source";

/// `routes` parted by the surface `config` makes each, those shown the
/// section first; each part none of whose routes is left out.
pub(crate) fn parted(config: &Config, routes: Vec<RouteId>) -> Vec<(Surface, Vec<RouteId>)> {
    let mut parts: Vec<(Surface, Vec<RouteId>)> = Vec::new();
    for id in routes {
        let surface = config.surface(&id);
        match parts.iter_mut().find(|(part, _)| *part == surface) {
            Some((_, ids)) => ids.push(id),
            None => parts.push((surface, vec![id])),
        }
    }
    parts.sort_by_key(|(surface, _)| !advises(*surface));
    parts
}

/// The section `surface` is shown of `found`, withheld where the round
/// read none.
pub(crate) fn shown(found: Option<&Advisories>, surface: Surface) -> Advised {
    found.map_or_else(Advised::withheld, |found| Advised::to(found, surface))
}

/// The advisories `advising` reads of every source `view` reads, after its
/// rows `pulls`, leaving `kept` of the run's requests for the posts: only
/// those of the repositories the view holds, and each source whose read
/// failed, the run's budget ending included, named unread with why, never
/// stopping the report.
pub(crate) async fn read<S: Store, C: Clock, G: Signer, H: Http>(
    (connected, ports): (&Connected<H>, &Ports<'_, S, C, G>),
    (view, advising, kept): (&ViewConfig, Advising, usize),
    pulls: &[Pull],
) -> Advisories {
    let budget = ports.budget.keeping(kept);
    let now = ports.clock.now();
    let rows = herald_core::rows(&view.view, pulls, now);
    let mut shown: Vec<&str> = rows
        .iter()
        .map(|row| row.pull.repository.as_str())
        .collect();
    shown.sort_unstable();
    shown.dedup();
    let mut found = Advisories::none(now);
    for name in &view.sources {
        let unread = |reason: String| Unread {
            repository: name.as_str().to_owned(),
            visibility: Visibility::Private,
            reason,
        };
        let Some((queue, http)) = connected
            .sources
            .iter()
            .find(|(source, _)| source.name == *name)
            .and_then(|(source, http)| source.queue.as_ref().map(|queue| (queue, http)))
        else {
            found.unread.push(unread(UNPOLLED.to_owned()));
            continue;
        };
        let members = if queue.scoped() {
            match crate::scope::members(ports.store, name).await {
                Ok(members) => Some(members),
                Err(stop) => {
                    found.unread.push(unread(stopped(&stop)));
                    continue;
                }
            }
        } else {
            None
        };
        let held: Vec<Listed> = members
            .iter()
            .flat_map(|(_, members)| members)
            .filter(|member| view.view.holds(&member.path))
            .cloned()
            .collect();
        let asked = Asked {
            most: view.most,
            scope: members.as_ref().map(|(owner, _)| Scope {
                owner: *owner,
                members: &held,
                narrowed: !view.view.holds_every(),
            }),
            stated: view.stated,
            files: false,
        };
        let http = &Budgeted::new(http, budget, ports.clock);
        match queue
            .advisories(http, (asked, &shown), (advising.within(&view.view), now))
            .await
        {
            Ok(read) => found.merge(read.within(&view.view, |path| queue.holds(path))),
            Err(unpolled) => found.unread.push(unread(http.unread_advisories(unpolled))),
        }
    }
    found
}

/// What `stop` says, as an unread source's reason: the store's answer, or
/// that no membership record is kept yet.
fn stopped(stop: &crate::announce::Stop) -> String {
    match stop {
        crate::announce::Stop::Source(reason) | crate::announce::Stop::Store(reason) => {
            format!("its members could not be read: {reason}")
        }
        _ => "its members could not be read".to_owned(),
    }
}
