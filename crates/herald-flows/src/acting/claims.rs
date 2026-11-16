//! The actors' names each route's own actor and each project hold, as
//! doctor reads them (ADR-0036).

use std::collections::BTreeMap;

use herald_config::Config;
use herald_core::SourceName;

use super::{Claim, Claimant, Projects};
use crate::prepare::Connected;

/// One label of a holder of an actor's name, as doctor names it, and
/// whether an owner's member holds it.
type Label = (String, bool);

/// Every actor's name, by its domain and name, with what holds it.
#[derive(Debug, Default)]
pub(crate) struct Claimed(BTreeMap<(String, String), BTreeMap<Claim, Vec<Label>>>);

impl Claimed {
    /// What `config`'s routes' own actors and the projects of their sources
    /// and apps hold.
    pub(crate) fn of_config(config: &Config) -> Self {
        let mut claimed = Self::default();
        for (index, route) in config.routes.iter().enumerate() {
            let Some(domain) = config
                .destinations
                .get(&route.route.destination)
                .and_then(|destination| destination.actor_domain())
            else {
                continue;
            };
            if let Some(name) = route.options.actor() {
                let claim = Claim::route(&route.route.destination);
                claimed.hold((domain, name), claim, own(index));
            }
            for (source, project) in Projects::of(config, route).iter().flat_map(Projects::named) {
                if let Some(Ok(name)) = route.options.names(project) {
                    let claim = Claim::source(source, project);
                    claimed.hold((domain, &name), claim, named(source, project, index));
                }
            }
        }
        claimed
    }

    /// What `connected`'s routes' own actors and the projects of their
    /// sources and apps hold.
    pub(crate) fn of_connected<H>(connected: &Connected<H>) -> Self {
        let mut claimed = Self::default();
        for (index, (route, _)) in connected.routes.iter().enumerate() {
            let Some(actor) = route.reached.federates() else {
                continue;
            };
            let Some(projects) = &route.projects else {
                let claim = Claim::route(&route.config.route.destination);
                claimed.hold((actor.domain(), actor.actor()), claim, own(index));
                continue;
            };
            for (source, project) in projects.named() {
                if let Some(Ok(name)) = actor.names(project) {
                    let claim = Claim::source(source, project);
                    claimed.hold(
                        (actor.domain(), &name),
                        claim,
                        named(source, project, index),
                    );
                }
            }
        }
        claimed
    }

    /// `claim` holding the actor `name` on `domain`, as `label` names it.
    pub(crate) fn hold(&mut self, (domain, name): (&str, &str), claim: Claim, label: Label) {
        self.0
            .entry((domain.to_owned(), name.to_owned()))
            .or_default()
            .entry(claim)
            .or_default()
            .push(label);
    }

    /// Each name more than one holds, as a mistake: only those an owner's
    /// member holds where `members` says so.
    pub(crate) fn collisions(&self, members: bool) -> Vec<String> {
        self.0
            .iter()
            .filter(|(_, held)| {
                held.len() > 1
                    && (!members || held.values().flatten().any(|(_, member)| *member))
            })
            .map(|((domain, name), held)| {
                let labels: Vec<&str> = held
                    .values()
                    .flatten()
                    .map(|(label, _)| label.as_str())
                    .collect();
                let routes = held
                    .keys()
                    .filter(|claim| matches!(claim.claimant, Claimant::Route(_)))
                    .count();
                let then = if routes == 1 {
                    "the route's own actor keeps it, and no project naming it is announced on its route; change the template or the route's actor"
                } else {
                    "herald serves it as none of theirs, and announces no project naming it on its route; give each its own name"
                };
                format!(
                    "The actor {name}@{domain} is named by {}; {then}.",
                    labels.join(", ")
                )
            })
            .collect()
    }
}

/// How doctor names route `index`'s own actor.
fn own(index: usize) -> Label {
    (format!("routes[{index}].actor"), false)
}

/// How doctor names `source`'s `project` on route `index`.
fn named(source: &SourceName, project: &str, index: usize) -> Label {
    (
        format!("{}'s project {project} on routes[{index}]", source.as_str()),
        false,
    )
}
