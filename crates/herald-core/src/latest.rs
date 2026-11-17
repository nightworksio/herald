//! What each project last announced, kept so a command can answer with it
//! without asking its source (ADR-0016).

use std::collections::{BTreeMap, BTreeSet};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::event::Event;
use crate::key::{Identity, RouteId};
use crate::kind::{Attribute, Kind, prerelease_in, version_in};

/// One event, as a command shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shown {
    /// What kind of thing it announced.
    pub kind: Kind,
    /// Its title.
    pub title: String,
    /// Its body, in Markdown.
    pub body: String,
    /// Its named links.
    pub links: BTreeMap<String, String>,
    /// Its kind's attributes, by name.
    pub attributes: BTreeMap<String, Attribute>,
    /// When it happened.
    pub occurred: Timestamp,
    /// What makes it that event, where it was kept with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<Identity>,
}

impl Shown {
    /// `event`, as a command shows it.
    #[must_use]
    pub fn of(event: &Event) -> Self {
        Self {
            kind: event.kind().clone(),
            title: event.title().to_owned(),
            body: event.body().to_owned(),
            links: event.links().clone(),
            attributes: event.attributes().clone(),
            occurred: event.occurred(),
            identity: Some(event.identity().clone()),
        }
    }

    /// Whether it is a pre-release.
    #[must_use]
    pub fn prerelease(&self) -> bool {
        prerelease_in(&self.attributes)
    }

    /// Whether it shows the event `identity` names.
    #[must_use]
    pub fn is(&self, identity: &Identity) -> bool {
        self.identity.as_ref() == Some(identity)
    }

    /// The version, where its kind carries one.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        version_in(&self.attributes)
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
        let stable = (!shown.prerelease()).then(|| shown.clone());
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

    /// What the project last announced once its release `identity` was
    /// withdrawn (ADR-0048): the newest it keeps falls back to the stable
    /// one where that is another release, a withdrawn stable release is no
    /// longer kept, and none is left where nothing else is kept.
    #[must_use]
    pub fn without(mut self, identity: &Identity) -> Option<Self> {
        let withdrawn = |shown: &Shown| shown.is(identity);
        if self.stable.as_ref().is_some_and(withdrawn) {
            self.stable = None;
        }
        if withdrawn(&self.newest) {
            self.newest = self.stable.clone()?;
        }
        Some(self)
    }

    /// Whether it keeps the event `identity` names, as its newest or its
    /// stable event.
    #[must_use]
    pub fn shows(&self, identity: &Identity) -> bool {
        self.newest.is(identity)
            || self
                .stable
                .as_ref()
                .is_some_and(|stable| stable.is(identity))
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
