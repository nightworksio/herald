//! What each project last announced, kept so a command can answer with it
//! without asking its source (ADR-0016).

use std::collections::{BTreeMap, BTreeSet};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::event::{Event, Kind};
use crate::key::RouteId;

/// One event, as a command shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shown {
    /// What kind of thing it announced.
    pub kind: Kind,
    /// The version, where it has one.
    pub version: Option<String>,
    /// Its title.
    pub title: String,
    /// Its body, in Markdown.
    pub body: String,
    /// Its named links.
    pub links: BTreeMap<String, String>,
    /// Whether it is a pre-release.
    pub prerelease: bool,
    /// When it happened.
    pub occurred: Timestamp,
}

impl Shown {
    /// `event`, as a command shows it.
    #[must_use]
    pub fn of(event: &Event) -> Self {
        Self {
            kind: event.kind(),
            version: event.version().map(str::to_owned),
            title: event.title().to_owned(),
            body: event.body().to_owned(),
            links: event.links().clone(),
            prerelease: event.prerelease(),
            occurred: event.occurred(),
        }
    }
}

/// What a project last announced: the newest event, the newest that is not
/// a pre-release, and the routes it was announced on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Latest {
    /// The project.
    pub project: String,
    /// The newest event announced.
    pub newest: Shown,
    /// The newest event announced that is not a pre-release.
    pub stable: Option<Shown>,
    /// The routes its events were announced on.
    pub routes: BTreeSet<RouteId>,
}

impl Latest {
    /// What `project` last announced once `event` is announced on `route`,
    /// after `kept`: an event older than the one kept leaves it in place.
    #[must_use]
    pub fn after(kept: Option<Self>, event: &Event, route: &RouteId) -> Self {
        let shown = Shown::of(event);
        let stable = (!shown.prerelease).then(|| shown.clone());
        let Some(mut latest) = kept else {
            return Self {
                project: event.project().to_owned(),
                newest: shown,
                stable,
                routes: BTreeSet::from([route.clone()]),
            };
        };
        if shown.occurred >= latest.newest.occurred {
            latest.newest = shown;
        }
        if let Some(stable) = stable
            && latest
                .stable
                .as_ref()
                .is_none_or(|kept| stable.occurred >= kept.occurred)
        {
            latest.stable = Some(stable);
        }
        latest.routes.insert(route.clone());
        latest
    }

    /// The newest event, or the newest that is not a pre-release where
    /// `prereleases` is false and there is one.
    #[must_use]
    pub fn shown(&self, prereleases: bool) -> &Shown {
        if prereleases {
            &self.newest
        } else {
            self.stable.as_ref().unwrap_or(&self.newest)
        }
    }
}

#[cfg(test)]
mod tests;
