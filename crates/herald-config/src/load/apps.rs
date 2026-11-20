//! The HTTP API's apps (ADR-0007): read before the routes, which may name an
//! app as a source, and checked against those routes after; each with the
//! ID tokens it takes and the names it stands for (ADR-0064).

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use herald_core::{Glob, RouteId, SourceName};
use herald_kinds::{ConfiguredStore, Source};
use toml::Spanned;

use super::oidc::Written;
use super::route::Read;
use super::{Loader, Value};
use crate::config::App;
use crate::file::AppFile;
use crate::oidc::{Replay, Taking};
use crate::peers::Peer;

/// An app as its table names it, its routes not yet checked.
pub(super) struct Named {
    tokens: Vec<String>,
    pub(super) oidc: Vec<Written>,
    projects: Vec<Glob>,
    routes: Vec<Spanned<String>>,
    pub(super) peers: Vec<String>,
    replay: Replay,
    pub(super) span: Range<usize>,
}

impl Loader<'_> {
    /// Each app of `[apps]`, its name one no source of `sources` has, and
    /// each issuer it takes ID tokens of read from `store` where it keeps
    /// them.
    pub(super) fn apps(
        &mut self,
        value: Option<Value<'_>>,
        sources: &BTreeMap<SourceName, Source>,
        store: Option<&ConfiguredStore>,
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
            if app.projects.is_empty() && app.peers.is_empty() {
                self.error(
                    &span,
                    &format!("{path}.projects"),
                    format!(
                        "an app announces only the projects it names; give them, such as projects = [{}], or the peers it stands for",
                        super::list(&AppFile::PROJECTS)
                    ),
                );
                sound = false;
            }
            let mut projects = Vec::new();
            for (index, project) in app.projects.iter().enumerate() {
                match Glob::new(project) {
                    Ok(glob) => projects.push(glob),
                    Err(error) => {
                        self.error(
                            &span,
                            &format!("{path}.projects[{index}]"),
                            error.to_string(),
                        );
                        sound = false;
                    }
                }
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
            let does: BTreeSet<Taking> = [
                (!app.projects.is_empty(), Taking::Announce),
                (!app.peers.is_empty(), Taking::Push),
            ]
            .into_iter()
            .filter_map(|(does, taking)| does.then_some(taking))
            .collect();
            let replay = self.replay((&span, &path), app.replay, (store, !app.oidc.is_empty()));
            let oidc = self.oidc((&span, &path), app.oidc, (store, &does));
            if let (true, Some(oidc), Some(replay)) = (sound, oidc, replay) {
                apps.insert(
                    name,
                    Named {
                        tokens,
                        oidc,
                        projects,
                        routes: app.routes,
                        peers: app.peers,
                        replay,
                        span,
                    },
                );
            }
        }
        apps
    }

    /// How often an ID token of the app at `path`, taking any where
    /// `takes`, may be presented: as `written`, else once where `store`
    /// keeps deliveries, so it remembers each token taken, and until it
    /// expires where it keeps none.
    fn replay(
        &mut self,
        (span, path): (&Range<usize>, &str),
        written: Option<Replay>,
        (store, takes): (Option<&ConfiguredStore>, bool),
    ) -> Option<Replay> {
        let keeps = store.is_none_or(|store| store.keeps_deliveries());
        let why = match written {
            Some(_) if !takes => {
                "replay says how often an ID token is taken, and the app takes none; give oidc entries, or leave replay out".to_owned()
            }
            Some(Replay::Once) if !keeps => format!(
                "the {} store keeps no deliveries, so it cannot remember a token taken; choose a store that keeps records, or replay = \"until-expiry\"",
                store.map(|store| store.kind()).unwrap_or_default()
            ),
            Some(written) => return Some(written),
            None if keeps => return Some(Replay::Once),
            None => return Some(Replay::UntilExpiry),
        };
        self.error(span, &format!("{path}.replay"), why);
        None
    }

    /// Each app of `named`, announcing to the routes it names or, where it
    /// names none, to every route of `routes` that takes its events; an app
    /// announcing no project names no route and needs none. Each ID token
    /// it takes names `audience` where its entry names none.
    pub(super) fn app_routes(
        &mut self,
        named: BTreeMap<String, Named>,
        routes: &[Read],
        (audience, peers): (Option<&str>, &BTreeMap<String, Peer>),
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
            if let Some(route) = app.routes.first().filter(|_| app.projects.is_empty()) {
                self.error(
                    &route.span(),
                    &format!("apps.{name}.routes"),
                    "an app that announces no project announces to no route; name its projects, or leave routes out",
                );
                continue;
            }
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
            let oidc = self.audienced((&app.span, &name), app.oidc, (audience, peers));
            let (true, Some(oidc)) = (sound, oidc) else {
                continue;
            };
            let routes = if app.projects.is_empty() {
                Vec::new()
            } else if app.routes.is_empty() {
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
                    oidc,
                    projects: app.projects,
                    routes,
                    peers: app.peers.into_iter().collect(),
                    replay: app.replay,
                },
            );
        }
        apps
    }
}
