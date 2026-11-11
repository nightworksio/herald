//! The HTTP API's apps (ADR-0007): read before the routes, which may name an
//! app as a source, and checked against those routes after.

use std::collections::BTreeMap;
use std::ops::Range;

use herald_core::{RouteId, SourceName};
use herald_kinds::Source;
use toml::Spanned;

use super::route::Read;
use super::{Loader, Value};
use crate::config::App;
use crate::file::AppFile;

/// An app as its table names it, its routes not yet checked.
pub(super) struct Named {
    tokens: Vec<String>,
    projects: Vec<String>,
    routes: Vec<Spanned<String>>,
    span: Range<usize>,
}

impl Loader<'_> {
    /// Each app of `[apps]`, its name one no source of `sources` has.
    pub(super) fn apps(
        &mut self,
        value: Option<Value<'_>>,
        sources: &BTreeMap<SourceName, Source>,
    ) -> BTreeMap<String, Named> {
        let mut apps = BTreeMap::new();
        let mut held: BTreeMap<String, String> = BTreeMap::new();
        for (name, value) in self.named(value, "apps") {
            let path = format!("apps.{name}");
            let span = value.span();
            let Some(app) = self.typed::<AppFile>(value, &path) else {
                continue;
            };
            let mut sound = true;
            if app.projects.is_empty() {
                self.error(
                    &span,
                    &format!("{path}.projects"),
                    format!(
                        "an app announces only the projects it names; give them, such as projects = [{}]",
                        super::list(&AppFile::PROJECTS)
                    ),
                );
                sound = false;
            }
            if sources.contains_key(&SourceName::new(&name)) {
                self.error(
                    &span,
                    &path,
                    format!(
                        "a source is named \"{name}\" too; a route names sources and apps alike, so give each its own name"
                    ),
                );
                sound = false;
            }
            let mut tokens = Vec::new();
            for token in &app.tokens {
                if let Some(hash) = AppFile::hash(token.get_ref()) {
                    if let Some(holder) = held.insert(hash.to_owned(), path.clone()) {
                        self.error(
                            &token.span(),
                            &format!("{path}.tokens"),
                            format!(
                                "{holder}.tokens lists this token too; give each app a token of its own, as herald token new prints it"
                            ),
                        );
                        sound = false;
                    }
                    tokens.push(hash.to_owned());
                } else {
                    self.error(
                        &token.span(),
                        &format!("{path}.tokens"),
                        "a token is written sha256: and its SHA-256 in 64 lowercase hex digits, as herald token new prints it",
                    );
                    sound = false;
                }
            }
            if sound {
                apps.insert(
                    name,
                    Named {
                        tokens,
                        projects: app.projects,
                        routes: app.routes,
                        span,
                    },
                );
            }
        }
        apps
    }

    /// Each app of `named`, announcing to the routes it names or, where it
    /// names none, to every route of `routes` that takes its events.
    pub(super) fn app_routes(
        &mut self,
        named: BTreeMap<String, Named>,
        routes: &[Read],
    ) -> BTreeMap<String, App> {
        let mut apps = BTreeMap::new();
        for (name, app) in named {
            let source = SourceName::new(&name);
            let taking = |id: &RouteId| {
                routes.iter().any(|read| {
                    read.config.route.id == *id
                        && read.config.route.filter.sources.contains(&source)
                })
            };
            let mut sound = true;
            for route in &app.routes {
                let id = RouteId::new(route.get_ref());
                let why = if !routes.iter().any(|read| read.config.route.id == id) {
                    let ids: Vec<&str> = routes
                        .iter()
                        .map(|read| read.config.route.id.as_str())
                        .collect();
                    format!(
                        "no route has the id \"{}\"; the routes are {}",
                        id.as_str(),
                        super::list(&ids)
                    )
                } else if !taking(&id) {
                    format!(
                        "the route \"{}\" takes no event of \"{name}\"; name the app in its sources",
                        id.as_str()
                    )
                } else {
                    continue;
                };
                self.error(&route.span(), &format!("apps.{name}.routes"), why);
                sound = false;
            }
            if !sound {
                continue;
            }
            let routes = if app.routes.is_empty() {
                let taken: Vec<RouteId> = routes
                    .iter()
                    .map(|read| read.config.route.id.clone())
                    .filter(|id| taking(id))
                    .collect();
                if taken.is_empty() {
                    self.error(
                        &app.span,
                        &format!("apps.{name}"),
                        format!(
                            "no route names the app \"{name}\" among its sources, so it announces nothing; add \"{name}\" to a route's sources"
                        ),
                    );
                    continue;
                }
                taken
            } else {
                app.routes
                    .iter()
                    .map(|route| RouteId::new(route.get_ref()))
                    .collect()
            };
            apps.insert(
                name,
                App {
                    tokens: app.tokens,
                    projects: app.projects,
                    routes,
                },
            );
        }
        apps
    }
}
