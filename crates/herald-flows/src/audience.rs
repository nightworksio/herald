//! Who may read each place a view's advisories section would reach,
//! checked once a run for each place right before a section is first sent
//! there, through the run's budget: a route is shown the section only where
//! it says `private = true` and herald checked that its place is not
//! publicly readable right now, or the operator vouched for a place herald
//! cannot check (ADR-0067).

use std::collections::BTreeMap;

use herald_config::Config;
use herald_core::{DestinationName, RouteId};
use herald_kinds::Checking;
use herald_ports::{Clock, DynHttp, Http, Signer, Store};

use crate::announce::Ports;
use crate::budget::Budgeted;
use crate::prepare::Connected;

/// Why a route this run does not reach, or whose kind is never sent one,
/// is withheld a section.
const UNSENT: &str = "this run does not reach the route, or its kind of destination is never sent an advisories section";

/// What the run found of each place it checked, by its destination and
/// the place its options write: why it withholds a section there, or none
/// where it is shown one. A place whose options write nothing is asked
/// again for each route, never taken for another's.
pub(crate) type Audiences = BTreeMap<(DestinationName, String), Option<String>>;

/// Why the route `id` is withheld an advisories section right now, its
/// place asked through the run's budget, leaving `kept` of its requests
/// for the posts still waiting, where `audiences` holds no answer for it
/// yet; none where it is shown one.
pub(crate) async fn withheld<S: Store, C: Clock, G: Signer, H: Http>(
    (config, connected, ports): (&Config, &Connected<H>, &Ports<'_, S, C, G>),
    (id, kept): (&RouteId, usize),
    audiences: &mut Audiences,
) -> Option<String> {
    let found = connected
        .routes
        .iter()
        .find(|(route, _)| route.config.route.id == *id)
        .and_then(|(route, http)| {
            let destination = config.destinations.get(&route.config.route.destination)?;
            Some((route, http, destination.checking(&route.config.options)))
        });
    let Some((route, http, Checking::Asked { .. })) = found else {
        return found
            .and_then(|(route, _, checking)| {
                let vouched = route.config.vouched;
                checking
                    .unchecked()
                    .map(|why| (!vouched).then(|| why.to_owned()))
            })
            .unwrap_or_else(|| Some(UNSENT.to_owned()));
    };
    let place = route
        .config
        .options
        .written()
        .map(|written| (route.config.route.destination.clone(), written));
    if let Some(known) = place.as_ref().and_then(|place| audiences.get(place)) {
        return known.clone();
    }
    let budgeted = Budgeted::new(http, ports.budget.keeping(kept), ports.clock);
    let audience = route.reached.audience(&budgeted as &dyn DynHttp).await;
    let why = audience.withheld().map(ToOwned::to_owned);
    if let Some(place) = place {
        audiences.insert(place, why.clone());
    }
    why
}
