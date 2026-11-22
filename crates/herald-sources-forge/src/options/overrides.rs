//! What an owner source's overrides give each repository a glob names.

use std::collections::BTreeSet;

use herald_core::{Glob, Reach, RouteId, Zoning};

use super::{Override, Sets, SourceOptions, sets_areas, sets_routes, sets_routes_only};

impl SourceOptions {
    /// The override of the narrowest glob of `repositories` naming the
    /// repository `name` that sets the key `sets` reads.
    pub(super) fn narrowest(&self, name: &str, sets: Sets) -> Option<&Override> {
        self.repositories
            .iter()
            .filter(|(_, chosen)| sets(chosen))
            .filter_map(|(glob, chosen)| Some((Glob::new(glob).ok()?, chosen)))
            .filter(|(glob, _)| glob.matches(name))
            .max_by_key(|(glob, _)| glob.specificity())
            .map(|(_, chosen)| chosen)
    }

    /// The routes the repository `name` names by the narrowest glob that
    /// sets each key, and whether they are its only routes.
    #[must_use]
    pub fn reach_of(&self, name: &str) -> Reach {
        Reach {
            routes: self
                .narrowest(name, sets_routes)
                .and_then(|chosen| chosen.routes.as_ref())
                .into_iter()
                .flatten()
                .map(RouteId::new)
                .collect(),
            only: self
                .narrowest(name, sets_routes_only)
                .and_then(|chosen| chosen.routes_only)
                .unwrap_or_default(),
        }
    }

    /// Each route an override names, with where it names it.
    #[must_use]
    pub fn routed(&self) -> Vec<(String, String)> {
        self.repositories
            .iter()
            .flat_map(|(glob, chosen)| {
                chosen
                    .routes
                    .iter()
                    .flatten()
                    .map(move |route| (format!("repositories.\"{glob}\".routes"), route.clone()))
            })
            .collect()
    }

    /// How the areas of the repository named `name`, its path's last
    /// part, are named: the narrowest override's way that sets one, or the
    /// source's (ADR-0071, ADR-0073).
    #[must_use]
    pub fn areas_of(&self, name: &str) -> Option<&Zoning> {
        self.narrowest(name, sets_areas)
            .and_then(|chosen| chosen.areas.as_ref())
            .or(self.areas.as_ref())
    }

    /// The name of each area the source or one of its overrides names.
    #[must_use]
    pub fn area_names(&self) -> BTreeSet<String> {
        self.areas
            .iter()
            .chain(
                self.repositories
                    .values()
                    .filter_map(|chosen| chosen.areas.as_ref()),
            )
            .flat_map(Zoning::names)
            .map(str::to_owned)
            .collect()
    }
}
