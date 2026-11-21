//! herald's ActivityPub actors found by name or id: a route's one actor, or,
//! where the route names its actor from the project, each project's own
//! (ADR-0036). A name more than one source, member or app names, or that a
//! route's own actor holds, is served and announced as no project's.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::Arc;

use herald_config::{Config, RouteConfig};
use herald_core::{DestinationName, Listed, Match, RouteId, SourceName, Visibility};
use herald_kinds::{Expand, Federates, Polling};
use herald_ports::{Cursors, Store, StoreError, Subscriptions};
use jiff::Timestamp;

use crate::announce::{Outcome, Stop};
use crate::poll::bounded;
use crate::prepare::{Connected, PreparedRoute};

mod claims;

pub(crate) use claims::Claimed;

/// What names an actor: a route's own actor on a destination, or a source,
/// app or owner's member announcing as a project.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Claimant {
    /// The one actor a route on the destination announces as.
    Route(DestinationName),
    /// A source or an app.
    Source(SourceName),
    /// The member of an owner source, by its forge id.
    Member(SourceName, String),
}

impl Claimant {
    /// The source or app it announces from, where it is no route's actor.
    fn source(&self) -> Option<&SourceName> {
        match self {
            Self::Route(_) => None,
            Self::Source(source) | Self::Member(source, _) => Some(source),
        }
    }
}

/// A claim of an actor's name by a project.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Claim {
    /// What names it.
    pub(crate) claimant: Claimant,
    /// The project it announces as.
    pub(crate) project: String,
}

impl Claim {
    /// The claim of the one actor a route on `destination` announces as.
    pub(crate) fn route(destination: &DestinationName) -> Self {
        Self {
            claimant: Claimant::Route(destination.clone()),
            project: String::new(),
        }
    }

    /// The claim of `source`, a source or app, announcing as `project`.
    pub(crate) fn source(source: &SourceName, project: &str) -> Self {
        Self {
            claimant: Claimant::Source(source.clone()),
            project: project.to_owned(),
        }
    }
}

/// What a route announcing as each project's own actor names its actors
/// from: the projects its sources and apps name, the owner sources whose
/// members join them, whether the route names each, whether members not
/// everyone may see join, and what of an event the route matches.
#[derive(Debug, Clone)]
pub(crate) struct Projects {
    named: Vec<(SourceName, String)>,
    owners: Vec<(SourceName, Arc<dyn Expand>, bool)>,
    unlisted: bool,
    id: RouteId,
    matching: Match,
}

impl Projects {
    /// The projects `route` of `config` names its actors from, where it
    /// announces each project as an actor of its own: those of its sources
    /// and of the owner sources whose members name it, and the projects its
    /// apps name one by one, each its match may take.
    pub(crate) fn of(config: &Config, route: &RouteConfig) -> Option<Self> {
        route.options.names("").map(drop)?;
        let id = &route.route.id;
        let matching = &route.route.filter.matching;
        let taking = &route.route.filter.sources;
        let mut named = Vec::new();
        let mut owners = Vec::new();
        for (name, source) in &config.sources {
            let sourced = taking.contains(name);
            if !sourced && !source.names_route(id) {
                continue;
            }
            match source.poller(None, false) {
                Polling::Many(expand) => owners.push((name.clone(), expand, sourced)),
                Polling::One(_) | Polling::Pull(_) | Polling::Unpolled => named.extend(
                    source
                        .projects()
                        .into_iter()
                        .filter(|project| matching.admits(project, None))
                        .map(|project| (name.clone(), project)),
                ),
            }
        }
        for (app, held) in config
            .apps
            .iter()
            .filter(|(_, app)| app.routes.contains(id))
        {
            let app = SourceName::new(app);
            named.extend(
                held.named()
                    .filter(|project| matching.admits(project, None))
                    .map(|project| (app.clone(), project.to_owned())),
            );
        }
        Some(Self {
            named,
            owners,
            unlisted: bounded(config, Visibility::Private).is_none_or(|routes| routes.contains(id)),
            id: id.clone(),
            matching: matching.clone(),
        })
    }

    /// Whether the member `repository` of an owner `expand` expands to,
    /// announcing as `project`, reaches the route, which names the owner
    /// where `sourced`.
    pub(crate) fn takes(
        &self,
        (expand, sourced): (&dyn Expand, bool),
        repository: &Listed,
        project: &str,
    ) -> bool {
        self.sees(repository.visibility)
            && expand.reach(repository).reaches(&self.id, sourced)
            && self
                .matching
                .admits(project, Some(&expand.origin(repository)))
    }

    /// Whether the route names the owner source `owner`, where it takes the
    /// owner's members at all.
    pub(crate) fn owner(&self, owner: &SourceName) -> Option<bool> {
        self.owners
            .iter()
            .find(|(name, _, _)| name == owner)
            .map(|(_, _, sourced)| *sourced)
    }

    /// Whether a member of `visibility` reaches the route.
    pub(crate) fn sees(&self, visibility: Visibility) -> bool {
        self.unlisted || visibility == Visibility::Public
    }

    /// Every project its sources and apps name, each with the source or app
    /// naming it, beside the members of its owners.
    pub(crate) fn named(&self) -> impl Iterator<Item = (&SourceName, &str)> {
        self.named
            .iter()
            .map(|(source, project)| (source, project.as_str()))
    }

    /// How the actor `name` of `actor`, served on `destination`, stands at
    /// `now`, as `store` keeps members and followers, with every project
    /// naming it: none where no project names it and no follower kept
    /// follows it.
    async fn standing<S: Store>(
        &self,
        (actor, destination): (&dyn Federates, &DestinationName),
        name: &str,
        store: &S,
        now: Timestamp,
    ) -> Result<Option<(Standing, BTreeSet<Claim>)>, StoreError> {
        let names = |project: &str| {
            actor
                .names(project)
                .is_some_and(|named| named.as_deref() == Ok(name))
        };
        let mut claims: BTreeSet<Claim> = self
            .named()
            .filter(|(_, project)| names(project))
            .map(|(source, project)| Claim::source(source, project))
            .collect();
        let mut live = !claims.is_empty();
        let mut kept = None;
        for (owner, expand, sourced) in &self.owners {
            for member in store.cursors().members(owner).await? {
                let member = member.value;
                let Some(project) = expand.project(&member.repository).ok().filter(|project| {
                    names(project)
                        && self.takes((expand.as_ref(), *sourced), &member.repository, project)
                }) else {
                    continue;
                };
                claims.insert(Claim {
                    claimant: Claimant::Member(owner.clone(), member.repository.id),
                    project,
                });
                match member.left {
                    None => live = true,
                    Some(left) => {
                        let within = left
                            .checked_add(expand.keep_members())
                            .is_ok_and(|until| until >= now);
                        kept = Some(kept == Some(true) || within);
                    }
                }
            }
        }
        if live {
            return Ok(Some((Standing::Live, claims)));
        }
        let followed = store
            .subscriptions()
            .subscribers(destination, name)
            .await?
            .iter()
            .any(|follower| follower.pending.is_none() && actor.admits(&follower.subscriber));
        let standing = match (kept, followed) {
            (Some(true), true) => Standing::Kept,
            (Some(_), _) | (None, true) => Standing::Gone,
            (None, false) => return Ok(None),
        };
        Ok(Some((standing, claims)))
    }
}

/// How one of herald's actors stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Standing {
    /// It announces, and takes follows.
    Live,
    /// Its project left its owner's selection; it keeps the followers it
    /// has, takes no new follows, and announces nothing.
    Kept,
    /// It is gone: its project left with no follower kept, or longer ago
    /// than its owner keeps members.
    Gone,
}

/// The actor asked for.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Wanted<'a> {
    /// By its name, on the domain `host`.
    Named {
        /// Its name.
        name: &'a str,
        /// The domain its handle is on.
        host: &'a str,
    },
    /// By its name, as `destination`'s.
    Of {
        /// Its name.
        name: &'a str,
        /// The destination serving it.
        destination: &'a DestinationName,
    },
    /// By its id.
    Id(&'a str),
}

impl<'a> Wanted<'a> {
    /// The name of the actor wanted, where `actor`, which `route` reaches,
    /// may serve it.
    fn name(self, route: &PreparedRoute, actor: &dyn Federates) -> Option<&'a str> {
        match self {
            Self::Named { name, host } => (actor.domain() == host).then_some(name),
            Self::Of { name, destination } => {
                (&route.config.route.destination == destination).then_some(name)
            }
            Self::Id(id) => actor.name_in(id),
        }
    }
}

/// How a found actor announces.
enum Announcing<'a> {
    /// As its route's one actor.
    Route(&'a dyn Federates),
    /// As one project's own actor.
    Named(Arc<dyn Federates>),
}

/// One of herald's actors, found, with the route serving it and the `Http`
/// it reaches inboxes through.
pub(crate) struct Acting<'a, H> {
    /// The route serving it.
    pub(crate) route: &'a PreparedRoute,
    /// What reaches inboxes.
    pub(crate) http: &'a H,
    /// How it stands.
    pub(crate) standing: Standing,
    announcing: Announcing<'a>,
}

impl<H> Acting<'_, H> {
    /// The actor.
    pub(crate) fn actor(&self) -> &dyn Federates {
        match &self.announcing {
            Announcing::Route(actor) => *actor,
            Announcing::Named(actor) => actor.as_ref(),
        }
    }

    /// The destination serving it.
    pub(crate) fn destination(&self) -> &DestinationName {
        &self.route.config.route.destination
    }
}

/// What names an actor: each route's own actor of that name, each route
/// naming it from a project with the projects naming it, and the store's
/// error where a route's members could not be read.
struct Holders<'a, H> {
    fixed: Vec<Acting<'a, H>>,
    projected: Vec<Acting<'a, H>>,
    claims: BTreeSet<Claim>,
    unread: Option<StoreError>,
}

/// What `connected`'s routes make of the actor `wanted`, as `store` keeps
/// members and followers at `now`.
async fn claimed<'a, S: Store, H>(
    connected: &'a Connected<H>,
    store: &S,
    wanted: Wanted<'_>,
    now: Timestamp,
) -> Holders<'a, H> {
    let mut found = Holders {
        fixed: Vec::new(),
        projected: Vec::new(),
        claims: BTreeSet::new(),
        unread: None,
    };
    let serving = connected
        .routes
        .iter()
        .filter_map(|(route, http)| Some((route, http, route.reached.federates()?)));
    for (route, http, actor) in serving {
        let Some(name) = wanted.name(route, actor) else {
            continue;
        };
        let destination = &route.config.route.destination;
        let Some(projects) = connected.projects.get(&route.config.route.id) else {
            if actor.actor() == name {
                found.claims.insert(Claim::route(destination));
                found.fixed.push(Acting {
                    route,
                    http,
                    standing: Standing::Live,
                    announcing: Announcing::Route(actor),
                });
            }
            continue;
        };
        match projects
            .standing((actor, destination), name, store, now)
            .await
        {
            Ok(Some((standing, claims))) => {
                found.claims.extend(claims);
                found.projected.push(Acting {
                    route,
                    http,
                    standing,
                    announcing: Announcing::Named(actor.named(name)),
                });
            }
            Ok(None) => {}
            Err(error) => found.unread = found.unread.or(Some(error)),
        }
    }
    found
}

/// What a lookup of an actor found.
pub(crate) enum Found<'a, H> {
    /// The actor.
    Acting(Acting<'a, H>),
    /// No actor: more than one route's actor, or more than one project,
    /// names it, as this says.
    Ambiguous(String),
    /// No actor of that name.
    Missing,
}

/// Who holds each of `claims`, in a sentence.
fn holders(claims: &BTreeSet<Claim>) -> String {
    claims
        .iter()
        .map(|claim| match &claim.claimant {
            Claimant::Route(destination) => format!("the actor of {}", destination.as_str()),
            Claimant::Source(source) | Claimant::Member(source, _) => {
                format!("{}'s project {}", source.as_str(), claim.project)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Why a name `claims` hold is served as none of theirs.
fn ambiguous(claims: &BTreeSet<Claim>) -> String {
    format!(
        "the actor's name is held by {}, so herald serves none of them",
        holders(claims)
    )
}

/// The actor `wanted` among those `connected`'s routes serve, as `store`
/// keeps members and followers at `now`. A route's own actor holds its
/// name before any project, and a name two routes' actors on other
/// destinations, or two projects, hold is served as none; otherwise the
/// first that is not gone, else the first gone.
///
/// # Errors
///
/// When the store cannot be read for a route that could serve it, and no
/// route's own actor holds the name.
pub(crate) async fn found<'a, S: Store, H>(
    connected: &'a Connected<H>,
    store: &S,
    wanted: Wanted<'_>,
    now: Timestamp,
) -> Result<Found<'a, H>, StoreError> {
    let Holders {
        mut fixed,
        projected,
        claims,
        unread,
    } = claimed(connected, store, wanted, now).await;
    let routes: BTreeSet<Claim> = claims
        .iter()
        .filter(|claim| matches!(claim.claimant, Claimant::Route(_)))
        .cloned()
        .collect();
    if routes.len() > 1 {
        return Ok(Found::Ambiguous(ambiguous(&routes)));
    }
    if !fixed.is_empty() {
        return Ok(Found::Acting(fixed.swap_remove(0)));
    }
    if let Some(error) = unread {
        return Err(error);
    }
    if claims.len() > 1 {
        return Ok(Found::Ambiguous(ambiguous(&claims)));
    }
    let pick = projected
        .iter()
        .position(|acting| acting.standing != Standing::Gone)
        .unwrap_or_default();
    Ok(projected
        .into_iter()
        .nth(pick)
        .map_or(Found::Missing, Found::Acting))
}

/// `route` as it announces an event of `project`, from `source`: as the
/// actor `project` names, where the route announces as each project's own,
/// so long
/// as no route's own actor, and no other source, member or app, holds that
/// name among `connected`'s routes, as `store` keeps members at `now`.
///
/// # Errors
///
/// The outcome of the route where it does not announce: skipped where the
/// project names no actor or one another holds, and stopped where the
/// store cannot tell.
pub(crate) async fn acting<'a, S: Store, H>(
    (connected, store, now): (&Connected<H>, &S, Timestamp),
    route: &'a PreparedRoute,
    (source, project): (&SourceName, &str),
) -> Result<Cow<'a, PreparedRoute>, Outcome> {
    let Some((actor, named)) = route
        .reached
        .federates()
        .and_then(|actor| Some((actor, actor.names(project)?)))
    else {
        return Ok(Cow::Borrowed(route));
    };
    let name = named
        .map_err(|why| Outcome::Skipped(format!("the project {project} names no actor: {why}")))?;
    let wanted = Wanted::Named {
        name: &name,
        host: actor.domain(),
    };
    let found = claimed(connected, store, wanted, now).await;
    let others: BTreeSet<Claim> = found
        .claims
        .into_iter()
        .filter(|claim| claim.claimant.source() != Some(source) || claim.project != project)
        .collect();
    if !others.is_empty() {
        return Err(Outcome::Skipped(format!(
            "the project {project} names the actor {name}, which {} holds too",
            holders(&others)
        )));
    }
    if let Some(error) = found.unread {
        return Err(Outcome::Stopped {
            landed: Vec::new(),
            remaining: Vec::new(),
            reason: Stop::Store(error.to_string()),
        });
    }
    let mut acting = route.clone();
    acting.reached = actor.named(&name).reached();
    Ok(Cow::Owned(acting))
}
