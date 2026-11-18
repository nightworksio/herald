//! A release's posts withdrawn as the operator asks, or as a tombstone of
//! another herald's feed does (ADR-0049), what a withdrawal would remove,
//! and a withdrawal a run stopped part-way finished by a round (ADR-0048).

use herald_config::Config;
use herald_core::{
    Entry, HeldEvent, Identity, Key, Pending, RouteId, SourceName, Step, Visibility, withdrawal,
};
use herald_ports::{Clock, Http, Ledger, Signer, Store, Versioned, Waiting};

use crate::acting::acting;
use crate::announce::{Announced, Asked, Outcome, Ports, Run, Stop, stored};
use crate::poll::{Sweep, Swept};
use crate::prepare::{Connected, PreparedRoute};

/// Why a route announcing each project as an actor of its own withdraws
/// nothing the operator asks it to.
const ACTING: &str = "the route announces each project as an actor of its own, and the operator names no project, so only the forge's deletion of the release withdraws it";

/// Why a route announcing each project as an actor of its own withdraws
/// nothing a tombstone of another herald's feed withdraws.
const TOMBSTONED: &str = "the route announces each project as an actor of its own, and a tombstone names no project, so its post stays";

/// What a route of the operator's choice would withdraw of a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Withdrawable {
    /// The route.
    pub route: RouteId,
    /// Each post of the release the route keeps, by the identity it was
    /// announced under.
    pub posts: Vec<WithdrawablePost>,
    /// Why the route withdraws nothing, where it does not.
    pub kept: Option<String>,
}

/// One post a withdrawal would remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawablePost {
    /// The identity it was announced under.
    pub identity: Identity,
    /// The steps that delete it.
    pub steps: Vec<Step>,
    /// The group whose post it has its place in, where it was gathered.
    pub group: Option<String>,
}

/// The routes the operator names: `route`, or every route where none.
fn chosen<'a, H>(
    connected: &'a Connected<H>,
    route: Option<&RouteId>,
) -> impl Iterator<Item = &'a (PreparedRoute, H)> {
    connected
        .routes
        .iter()
        .filter(move |(prepared, _)| route.is_none_or(|route| prepared.config.route.id == *route))
}

/// Whether `route` announces each project as an actor of its own.
fn acts(route: &PreparedRoute) -> bool {
    route
        .reached
        .federates()
        .is_some_and(|actor| actor.names("").is_some())
}

/// `identity`'s posts withdrawn from `route`, or from every route where it
/// names none, whatever each route's `withdraw` says, as `run`.
pub async fn withdraw<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    (route, identity): (Option<&RouteId>, &Identity),
    run: &str,
) -> Vec<Announced> {
    let mut withdrawn = Vec::new();
    for (prepared, http) in chosen(connected, route) {
        let outcome = if acts(prepared) {
            Outcome::Skipped(ACTING.to_owned())
        } else {
            let running = running(config, ports, (prepared, http), run);
            running
                .withdraw(config, (identity, None), None, Asked::Operator)
                .await
        };
        withdrawn.push(Announced {
            route: prepared.config.route.id.clone(),
            outcome,
        });
    }
    withdrawn
}

/// What each route taking `source` did, as `run`, with `identity`, which
/// another herald's feed withdrew by a tombstone (ADR-0049): its posts
/// withdrawn as the route's `withdraw` says, where `source` announced them;
/// nothing where the route announced nothing of it, or another source did.
pub(crate) async fn tombstoned<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    (source, identity): (&SourceName, &Identity),
    run: &str,
) -> Vec<Announced> {
    let mut withdrawn = Vec::new();
    let taking = connected
        .routes
        .iter()
        .filter(|(prepared, _)| prepared.config.route.filter.sources.contains(source));
    for (prepared, http) in taking {
        let outcome = if acts(prepared) {
            Outcome::Skipped(TOMBSTONED.to_owned())
        } else {
            running(config, ports, (prepared, http), run)
                .withdraw(config, (identity, None), None, Asked::Tombstone(source))
                .await
        };
        withdrawn.push(Announced {
            route: prepared.config.route.id.clone(),
            outcome,
        });
    }
    withdrawn
}

/// What withdrawing `identity` from `route`, or from every route where it
/// names none, would remove, nothing sent.
///
/// # Errors
///
/// Where the ledger cannot be read.
pub async fn withdrawable<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    (route, identity): (Option<&RouteId>, &Identity),
) -> Result<Vec<Withdrawable>, Stop> {
    let mut every = Vec::new();
    for (prepared, http) in chosen(connected, route) {
        let route = prepared.config.route.id.clone();
        let kept = if acts(prepared) {
            Some(ACTING.to_owned())
        } else {
            crate::announce::undeletable(prepared).map(ToOwned::to_owned)
        };
        let mut kept_posts = Vec::new();
        if kept.is_none() {
            let running = running(config, ports, (prepared, http), "dry-run");
            kept_posts = posts(&running, identity).await?;
        }
        every.push(Withdrawable {
            route,
            posts: kept_posts,
            kept,
        });
    }
    Ok(every)
}

/// The posts of `identity` the route of `running` keeps, as a withdrawal
/// would remove them.
async fn posts<S: Store, C: Clock, G: Signer, H: Http>(
    running: &Run<'_, S, C, G, H>,
    identity: &Identity,
) -> Result<Vec<WithdrawablePost>, Stop> {
    let live = running.live(identity).await?;
    let mut posts = Vec::new();
    for target in [live.clone(), crate::announce::reposted(&live)] {
        let key = Key::derive(&running.route.config.route.id, &target);
        let kept = running
            .ports
            .store
            .ledger()
            .entry(&key)
            .await
            .map_err(stored)?;
        if let Some(entry) = kept
            .map(|kept| kept.value)
            .filter(|entry| entry.withdrawn.is_none())
        {
            posts.push(post(target, &entry));
        }
    }
    Ok(posts)
}

/// The post `entry` keeps of `identity`, as a withdrawal would remove it.
fn post(identity: Identity, entry: &Entry) -> WithdrawablePost {
    WithdrawablePost {
        identity,
        steps: withdrawal(entry),
        group: entry.group.clone(),
    }
}

/// A run of `route` within `config`, as `run`, for a release whose
/// visibility it does not know.
fn running<'a, S, C, G, H>(
    config: &'a Config,
    ports: &'a Ports<'a, S, C, G>,
    (route, http): (&'a PreparedRoute, &'a H),
    run: &'a str,
) -> Run<'a, S, C, G, H> {
    Run {
        ports,
        route,
        kinds: &config.kinds,
        http,
        holder: run,
        lease: config.announce.lease,
        seen: Visibility::Public,
    }
}

/// What the route `route_of` did, as `holder`, finishing the withdrawal of
/// `identity` the forge's deletion `deleted` began: as the actor of its
/// project where the route announces each project as one.
async fn resumed<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    (connected, ports): (&Connected<H>, &Ports<'_, S, C, G>),
    (prepared, http): (&PreparedRoute, &H),
    holder: &str,
    (identity, deleted): (&Identity, &HeldEvent),
) -> Outcome {
    let now = ports.clock.now();
    let found = (&deleted.source, &deleted.event);
    match acting((connected, ports.store, now), prepared, found).await {
        Ok(acted) => {
            let released = (identity, Some(deleted.event.project()));
            running(config, ports, (&acted, http), holder)
                .withdraw(config, released, Some(deleted.clone()), Asked::Operator)
                .await
        }
        Err(outcome) => outcome,
    }
}

/// What a round did with the withdrawal of `identity` from `route` its
/// record `kept` holds, as `run`: finished, from the release the forge
/// deleted where the record holds it; forgotten where the route is gone.
pub(crate) async fn round<S: Store, C: Clock, G: Signer, H: Http>(
    config: &Config,
    connected: &Connected<H>,
    ports: &Ports<'_, S, C, G>,
    run: &str,
    (route, identity): (&RouteId, &Identity),
    kept: Versioned<Pending>,
) -> Option<Swept> {
    let source = herald_core::SourceName::new(route.as_str());
    let swept = |outcome| Swept {
        source: source.clone(),
        member: None,
        outcome,
    };
    let Some((prepared, http)) = chosen(connected, Some(route)).next() else {
        let dropped = ports.store.waiting().drop_pending(&kept.value.hold).await;
        return dropped
            .err()
            .map(|error| swept(Sweep::Stopped(stored(error))));
    };
    let holder = format!("{run}/{}", route.as_str());
    let route_of = (prepared, http);
    let outcome = if let Some(deleted) = kept.value.events.first() {
        resumed(
            config,
            (connected, ports),
            route_of,
            &holder,
            (identity, deleted),
        )
        .await
    } else {
        running(config, ports, route_of, &holder)
            .withdraw(config, (identity, None), None, Asked::Operator)
            .await
    };
    if let Outcome::Skipped(_) = outcome
        && let Err(error) = ports.store.waiting().drop_pending(&kept.value.hold).await
    {
        return Some(swept(Sweep::Stopped(stored(error))));
    }
    Some(swept(Sweep::Withdrawn {
        identity: identity.clone(),
        announced: vec![Announced {
            route: route.clone(),
            outcome,
        }],
    }))
}
