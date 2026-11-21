//! Reading `herald.toml`: the loader each section is read with, and the
//! sections read in turn into a config.

use std::ops::Range;

use herald_core::KindSpecs;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use toml::Spanned;
use toml::de::{DeTable, DeValue, ValueDeserializer};

use herald_kinds::Fault;

use crate::config::Config;
use crate::error::{ConfigError, Errors};
use crate::example::explained;
use crate::kinds::{At, Kinds};
use crate::position::position;
use secrets::secret_keys;

pub(crate) type Value<'i> = Spanned<DeValue<'i>>;

fn list(names: &[&str]) -> String {
    names
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

fn contains(span: &Range<usize>, at: &Range<usize>) -> bool {
    span.start <= at.start && at.end <= span.end
}

fn path_at(base: &str, value: &Value<'_>, at: &Range<usize>) -> String {
    match value.get_ref() {
        DeValue::Table(table) => table
            .iter()
            .find(|(key, child)| contains(&key.span(), at) || contains(&child.span(), at))
            .map_or_else(
                || base.to_owned(),
                |(key, child)| path_at(&format!("{base}.{}", key.get_ref()), child, at),
            ),
        DeValue::Array(items) => items
            .iter()
            .enumerate()
            .find(|(_, item)| contains(&item.span(), at))
            .map_or_else(
                || base.to_owned(),
                |(index, item)| path_at(&format!("{base}[{index}]"), item, at),
            ),
        _ => base.to_owned(),
    }
}

pub(crate) struct Loader<'t> {
    file: &'t str,
    text: &'t str,
    kinds: &'t Kinds,
    specs: KindSpecs,
    errors: Vec<ConfigError>,
}

impl<'t> Loader<'t> {
    /// A loader of the file `file` holding `text`, reading kinds by
    /// `kinds`, with nothing found yet.
    fn new(file: &'t str, text: &'t str, kinds: &'t Kinds) -> Self {
        Self {
            file,
            text,
            kinds,
            specs: KindSpecs::default(),
            errors: Vec::new(),
        }
    }
}

impl Loader<'_> {
    fn error(&mut self, span: &Range<usize>, path: &str, message: impl Into<String>) {
        let (line, column) = position(self.text, span.start);
        self.errors.push(ConfigError {
            line,
            column,
            path: path.to_owned(),
            message: message.into(),
        });
    }

    /// `fault`, reported where the table at `at` writes it.
    pub(crate) fn fault(&mut self, at: At<'_>, fault: &Fault) {
        let path = match &fault.key {
            Some(key) => format!("{}.{key}", at.path),
            None => at.path.to_owned(),
        };
        self.error(at.span, &path, fault.message.clone());
    }

    pub(crate) fn typed<T: DeserializeOwned + JsonSchema>(
        &mut self,
        value: Value<'_>,
        path: &str,
    ) -> Option<T> {
        let mut typed = None;
        let explain = |message: &str, at: &str| {
            let within: Vec<&str> = at
                .strip_prefix(path)
                .unwrap_or_default()
                .split('.')
                .filter(|step| !step.is_empty())
                .map(|step| step.split_once('[').map_or(step, |(key, _)| key))
                .collect();
            explained::<T>(message, &within)
        };
        let clean = self.retrying(value, path, &explain, &mut |value| {
            T::deserialize(ValueDeserializer::from(value)).map(|read| typed = Some(read))
        });
        typed.filter(|_| clean)
    }

    fn retrying(
        &mut self,
        value: Value<'_>,
        path: &str,
        explain: &dyn Fn(&str, &str) -> String,
        attempt: &mut dyn FnMut(Value<'_>) -> Result<(), toml::de::Error>,
    ) -> bool {
        let mut value = value;
        let mut reported = false;
        loop {
            let span = value.span();
            let copy = value.clone();
            let Err(error) = attempt(value) else {
                return !reported;
            };
            let at = error.span().unwrap_or(span.clone());
            let at_path = path_at(path, &copy, &at);
            let message = explain(error.message(), &at_path);
            let DeValue::Table(mut table) = copy.clone().into_inner() else {
                self.error(&at, &at_path, message);
                return false;
            };
            let culprit = table
                .iter()
                .find(|(key, child)| contains(&key.span(), &at) || contains(&child.span(), &at))
                .map(|(key, _)| key.get_ref().to_string());
            let Some(key) = culprit else {
                if !reported {
                    self.error(&at, path, explain(error.message(), path));
                }
                return false;
            };
            self.error(&at, &at_path, message);
            table.remove(key.as_str());
            reported = true;
            value = Spanned::new(span, DeValue::Table(table));
        }
    }

    fn table<'i>(&mut self, value: Value<'i>, path: &str) -> Option<(Range<usize>, DeTable<'i>)> {
        let span = value.span();
        if let DeValue::Table(table) = value.into_inner() {
            return Some((span, table));
        }
        self.error(&span, path, format!("{path} is a table; write [{path}]"));
        None
    }

    /// The table `value` at `path`, parted into the keys `keys` names and
    /// the rest, with where the table is written.
    fn parted<'i>(
        &mut self,
        value: Value<'i>,
        path: &str,
        keys: &[&str],
    ) -> Option<(Value<'i>, Range<usize>, DeTable<'i>)> {
        let (span, table) = self.table(value, path)?;
        let (named, rest): (DeTable<'i>, DeTable<'i>) = table
            .into_iter()
            .partition(|(key, _)| keys.contains(&key.get_ref().as_ref()));
        Some((
            Spanned::new(span.clone(), DeValue::Table(named)),
            span,
            rest,
        ))
    }

    fn kinded<'i>(
        &mut self,
        value: Value<'i>,
        path: &str,
        kinds: &[&str],
    ) -> Option<(String, Value<'i>)> {
        self.kinded_table(value, path, kinds)
            .map(|(kind, span, table)| (kind, Spanned::new(span, DeValue::Table(table))))
    }

    /// The kind `value`'s table names, one of `kinds`, and the rest of the
    /// table with where it is written.
    fn kinded_table<'i>(
        &mut self,
        value: Value<'i>,
        path: &str,
        kinds: &[&str],
    ) -> Option<(String, Range<usize>, DeTable<'i>)> {
        let (span, mut table) = self.table(value, path)?;
        let Some(kind) = table.remove("kind") else {
            self.error(
                &span,
                path,
                format!("give its kind: kind = one of {}", list(kinds)),
            );
            return None;
        };
        let kind_span = kind.span();
        let kind_path = format!("{path}.kind");
        match kind.into_inner() {
            DeValue::String(name) if kinds.contains(&name.as_ref()) => {
                Some((name.into_owned(), span, table))
            }
            DeValue::String(name) => {
                self.error(
                    &kind_span,
                    &kind_path,
                    format!("there is no kind \"{name}\"; the kinds are {}", list(kinds)),
                );
                None
            }
            _ => {
                self.error(
                    &kind_span,
                    &kind_path,
                    format!("a kind is a string, one of {}", list(kinds)),
                );
                None
            }
        }
    }

    fn named<'i>(&mut self, value: Option<Value<'i>>, path: &str) -> Vec<(String, Value<'i>)> {
        let Some(value) = value else {
            return Vec::new();
        };
        let Some((_, table)) = self.table(value, path) else {
            return Vec::new();
        };
        table
            .into_iter()
            .map(|(name, value)| (name.into_inner().into_owned(), value))
            .collect()
    }

    /// Every mistake found, in the order the file writes them.
    fn failed(self) -> Errors {
        let mut errors = self.errors;
        errors.sort_by_key(|error| (error.line, error.column));
        Errors {
            file: self.file.to_owned(),
            errors,
        }
    }
}

/// The config `text` holds, read from the file named `file`, with each
/// template file and shared block list read through `read`.
///
/// # Errors
///
/// Every mistake in the file, each with its line, column, key and fix.
pub fn load(
    file: &str,
    text: &str,
    read: &dyn Fn(&str) -> Result<String, String>,
    kinds: &Kinds,
) -> Result<Config, Errors> {
    let mut loader = Loader::new(file, text, kinds);
    let (root, syntax) = DeTable::parse_recoverable(text);
    for error in syntax {
        loader.error(&error.span().unwrap_or(0..0), "", error.message());
    }
    let secret_keys = secret_keys(root.get_ref());
    let sections = loader.sections(root.into_inner());
    loader.version(sections.version);
    loader.specs = loader.declared(sections.kinds);
    let (store, kept) = loader.store(sections.store).unzip();
    let (retention, attempts) = kept.unwrap_or_default();
    let marker_key = loader.marker_key(sections.markers);
    let (federation, inherited) = loader.federation(sections.federation, store.as_ref());
    let peers = loader.peers(sections.peers, &federation.revoked);
    let mut destinations = loader.destinations(sections.destinations, &inherited, read);
    loader.feeds(store.as_ref(), &federation, &mut destinations);
    loader.pushes((store.as_ref(), &federation), &peers, &mut destinations);
    let (mut sources, source_spans) = loader.sources(sections.sources, read);
    loader.joined((&mut sources, &source_spans), &peers, &federation.revoked);
    let listed = loader.apps(sections.apps, &sources, store.as_ref());
    let makers = loader.credentials(&listed, (&peers, &federation), (&sources, &source_spans));
    loader.remembering((store.as_ref(), makers), (&mut sources, &source_spans));
    let templates = loader.templates(sections.templates, read);
    let models = loader.models(sections.models);
    let tasks = loader.tasks(sections.tasks, &models);
    let views = loader.views(sections.views, &sources, &tasks);
    let (boarded, boards) = loader.boards(sections.boards);
    let named = listed.keys().map(String::as_str).collect();
    let reports = loader.reports(sections.reports, boarded, &views, (&sources, &named));
    let rules = loader.rules(sections.rules, &views, (&sources, &named), &reports);
    let github_apps = loader.apps_of(
        sections.github_apps,
        store.as_ref(),
        (&sources, &source_spans),
    );
    let routes = loader.every_route(
        sections.routes,
        &destinations,
        &sources,
        (route::Others::of(&listed, &reports, &rules), boards),
        &templates,
    );
    loader.followed(store.as_ref(), &destinations, &routes);
    loader.answered(store.as_ref(), &destinations, &templates);
    let render = loader.render(sections.render);
    let compiled = loader.compiled(&templates, &routes, render);
    let server = loader.server_section(sections.server, store.as_ref());
    let audience = oidc::audience(&server, &federation);
    let apps = loader.app_routes(listed, &routes, (audience.as_deref(), &peers));
    let schedule = loader.schedule(sections.schedule);
    let http = loader.http_section(sections.http, read);
    loader.outlasted(&destinations, &http);
    loader.waited(&destinations);
    let announce = loader.announce(sections.announce);
    let run = loader.run(sections.run);
    let doctor = loader.doctor(sections.doctor);
    loader.exposed(&sources, &routes, doctor);
    loader.unstated(&routes, doctor);
    loader.unaddressed(&destinations, &routes);
    loader.routed((&sources, &source_spans), &routes, doctor);
    let (true, Some(store), Some(templates)) = (loader.errors.is_empty(), store, compiled) else {
        return Err(loader.failed());
    };
    written::apart(
        Config {
            store,
            retention,
            attempts,
            render,
            marker_key,
            destinations: destinations
                .into_iter()
                .map(|(name, (destination, _))| (name, destination))
                .collect(),
            sources,
            routes: routes.into_iter().map(|read| read.config).collect(),
            templates,
            apps,
            github_apps,
            views,
            reports,
            rules,
            models,
            tasks,
            schedule,
            server,
            federation,
            peers,
            http,
            announce,
            run,
            doctor,
            kinds: std::mem::take(&mut loader.specs),
            secret_keys,
        },
        loader,
        &source_spans,
    )
}

mod apps;
mod boards;
mod declared;
mod destinations;
mod federation;
mod feeds;
mod github_apps;
mod group;
mod models;
mod oidc;
mod pages;
mod peers;
mod pushes;
mod route;
mod secrets;
mod sections;
mod settings;
mod sources;
mod store;
mod templates;
mod views;
mod written;
