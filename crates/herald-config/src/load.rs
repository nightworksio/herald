use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use herald_core::{
    DestinationName, KindSpecs, RecordKind, Retention, RouteId, SourceName, TemplateName,
};
use serde::de::DeserializeOwned;
use toml::Spanned;
use toml::de::{DeTable, DeValue, ValueDeserializer};

use herald_kinds::{Destination, Fault, Source, Store, StoreKind};
use herald_store_git_branch::GitBranchKind;

use crate::config::{Config, Schedule};
use crate::error::{ConfigError, Errors};
use crate::file::{HttpFile, Keep, KeepFile, MarkersFile, ScheduleFile, ServerFile, TemplateFile};
use crate::kinds::{At, Kinds};
use crate::position::position;

pub(crate) type Value<'i> = Spanned<DeValue<'i>>;

/// The version of the config this herald reads.
const VERSION: i64 = 1;
/// The top-level keys of `herald.toml`.
const SECTIONS: [&str; 14] = [
    "version",
    "kinds",
    "store",
    "markers",
    "destinations",
    "sources",
    "routes",
    "templates",
    "apps",
    "schedule",
    "server",
    "http",
    "render",
    "announce",
];
/// The length of a SHA-256 in hex.
const HASH_DIGITS: usize = 64;

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
    text: &'t str,
    kinds: &'t Kinds,
    specs: KindSpecs,
    errors: Vec<ConfigError>,
}

/// The sections of the file, before each is read.
#[derive(Default)]
struct Sections<'i> {
    version: Option<Value<'i>>,
    kinds: Option<Value<'i>>,
    store: Option<Value<'i>>,
    markers: Option<Value<'i>>,
    destinations: Option<Value<'i>>,
    sources: Option<Value<'i>>,
    routes: Option<Value<'i>>,
    templates: Option<Value<'i>>,
    apps: Option<Value<'i>>,
    schedule: Option<Value<'i>>,
    server: Option<Value<'i>>,
    http: Option<Value<'i>>,
    render: Option<Value<'i>>,
    announce: Option<Value<'i>>,
}

/// A template's sources, with where each is written.
struct Written {
    title: Spanned<String>,
    body: Spanned<String>,
    body_path: String,
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

    pub(crate) fn typed<T: DeserializeOwned>(&mut self, value: Value<'_>, path: &str) -> Option<T> {
        let mut typed = None;
        let clean = self.retrying(value, path, &mut |value| {
            T::deserialize(ValueDeserializer::from(value)).map(|read| typed = Some(read))
        });
        typed.filter(|_| clean)
    }

    fn retrying(
        &mut self,
        value: Value<'_>,
        path: &str,
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
            let DeValue::Table(mut table) = copy.clone().into_inner() else {
                self.error(&at, &path_at(path, &copy, &at), error.message());
                return false;
            };
            let culprit = table
                .iter()
                .find(|(key, child)| contains(&key.span(), &at) || contains(&child.span(), &at))
                .map(|(key, _)| key.get_ref().to_string());
            let Some(key) = culprit else {
                if !reported {
                    self.error(&at, path, error.message());
                }
                return false;
            };
            self.error(&at, &path_at(path, &copy, &at), error.message());
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

    fn sections<'i>(&mut self, root: DeTable<'i>) -> Sections<'i> {
        let mut sections = Sections::default();
        for (key, value) in root {
            let slot = match key.get_ref().as_ref() {
                "version" => &mut sections.version,
                "kinds" => &mut sections.kinds,
                "store" => &mut sections.store,
                "markers" => &mut sections.markers,
                "destinations" => &mut sections.destinations,
                "sources" => &mut sections.sources,
                "routes" => &mut sections.routes,
                "templates" => &mut sections.templates,
                "apps" => &mut sections.apps,
                "schedule" => &mut sections.schedule,
                "server" => &mut sections.server,
                "http" => &mut sections.http,
                "render" => &mut sections.render,
                "announce" => &mut sections.announce,
                other => {
                    self.error(
                        &key.span(),
                        other,
                        format!(
                            "herald.toml has no key \"{other}\"; the keys are {}",
                            list(&SECTIONS)
                        ),
                    );
                    continue;
                }
            };
            *slot = Some(value);
        }
        sections
    }

    fn version(&mut self, value: Option<Value<'_>>) {
        let Some(value) = value else {
            self.error(&(0..0), "version", "give the config's version: version = 1");
            return;
        };
        let span = value.span();
        if self.typed::<i64>(value, "version") != Some(VERSION) {
            self.error(
                &span,
                "version",
                "this herald reads version 1 of the config; write version = 1",
            );
        }
    }

    fn store(&mut self, value: Option<Value<'_>>) -> Option<(Store, Retention)> {
        let Some(value) = value else {
            self.error(
                &(0..0),
                "store",
                "choose a store: [store] with kind = \"git-branch\" on GitHub Actions",
            );
            return None;
        };
        let kinds = self.kinds;
        let (kind, span, table) = self.kinded_table(value, "store", &kinds.store_kinds())?;
        let kind = kinds.store_named(&kind)?;
        if !kind.retained() {
            let store = kind.parse(self, Spanned::new(span, DeValue::Table(table)))?;
            return Some((store, Retention::default()));
        }
        let (rest, keep) = kept(span, table);
        let retention = self.retention(keep);
        let store = kind.parse(self, rest)?;
        Some((store, retention?))
    }

    fn routes<'i>(&mut self, value: Option<Value<'i>>) -> Vec<Value<'i>> {
        let Some(value) = value else {
            return Vec::new();
        };
        let span = value.span();
        if let DeValue::Array(items) = value.into_inner() {
            return items.into_iter().collect();
        }
        self.error(
            &span,
            "routes",
            "routes are an array of tables; write each as [[routes]]",
        );
        Vec::new()
    }

    fn retention(&mut self, keep: Value<'_>) -> Option<Retention> {
        let file = self.typed::<KeepFile>(keep, "store")?;
        let defaults = Retention::default();
        let deliveries = file.deliveries.map_or(defaults.deliveries, |kept| {
            if kept.get_ref().0 < Retention::LONGEST_RETRY {
                self.error(
                    &kept.span(),
                    "store.keep_deliveries",
                    format!(
                        "keep a delivery's id at least \"{}d\", longer than any sender goes on retrying it",
                        Retention::LONGEST_RETRY.as_hours() / 24
                    ),
                );
            }
            kept.into_inner().0
        });
        Some(Retention {
            deliveries,
            ledger: file.ledger.map_or(defaults.ledger, Keep::kept),
            notes: file.notes.map_or(defaults.notes, Keep::kept),
        })
    }

    /// Whether every session a destination holds open waits less than
    /// `[http] total` allows any answer.
    fn outlasted(
        &mut self,
        destinations: &BTreeMap<DestinationName, (Destination, Range<usize>)>,
        http: &herald_ports::Outbound,
    ) {
        let kinds = self.kinds;
        for (name, (destination, span)) in destinations {
            let Some(fault) = kinds
                .destination_named(destination.kind())
                .and_then(|kind| kind.outlasts(destination, http.total))
            else {
                continue;
            };
            let path = format!("destinations.{}", name.as_str());
            self.fault(At { path: &path, span }, &fault);
        }
    }

    /// Whether every wait a destination sets is long enough.
    fn waited(&mut self, destinations: &BTreeMap<DestinationName, (Destination, Range<usize>)>) {
        for (name, (destination, span)) in destinations {
            if let Some(Err(error)) = destination
                .waits_written()
                .map(|set| herald_core::Waits::check(&set))
            {
                self.error(span, &format!("destinations.{}", name.as_str()), error);
            }
        }
    }

    /// Whether every route that reaches followers or subscribers has a store
    /// that keeps them.
    fn followed(
        &mut self,
        store: Option<&Store>,
        destinations: &BTreeMap<DestinationName, (Destination, Range<usize>)>,
        routes: &[route::Read],
    ) {
        let Some(store) = store else {
            return;
        };
        let elsewhere = |kind| {
            store.kept_by(kind).unwrap_or_else(|| {
                format!(
                    "give a store that keeps them, such as \"{}\"",
                    GitBranchKind::NAME
                )
            })
        };
        for read in routes {
            let route = &read.config.route;
            let follows = destinations
                .get(&route.destination)
                .is_some_and(|(destination, _)| destination.acts());
            if (follows || route.delivery.mentions.subscribers)
                && !store.keeps(RecordKind::Subscriptions)
            {
                self.error(
                    &read.id_span,
                    &read.path,
                    format!(
                        "the {} store keeps no subscriptions, so this route has no followers or subscribers to reach; {}",
                        store.kind(),
                        elsewhere(RecordKind::Subscriptions)
                    ),
                );
            } else if follows && !store.keeps(RecordKind::Notes) {
                self.error(
                    &read.id_span,
                    &read.path,
                    format!(
                        "the {} store keeps no notes, which this route's actor serves its announcements as; {}",
                        store.kind(),
                        elsewhere(RecordKind::Notes)
                    ),
                );
            }
        }
    }

    /// Whether every destination that answers commands names a template
    /// there is, and has a store that keeps what its commands answer with.
    fn answered(
        &mut self,
        store: Option<&Store>,
        destinations: &BTreeMap<DestinationName, (Destination, Range<usize>)>,
        templates: &BTreeMap<TemplateName, Written>,
    ) {
        for (name, (destination, span)) in destinations {
            let Some(answering) = destination.answering() else {
                continue;
            };
            let path = format!("destinations.{}.interactions", name.as_str());
            if let Some(template) = &answering.template
                && !templates.contains_key(template)
            {
                let names: Vec<&str> = templates.keys().map(TemplateName::as_str).collect();
                self.error(
                    span,
                    &format!("{path}.template"),
                    format!(
                        "no template is named \"{}\"; the templates are {}",
                        template.as_str(),
                        list(&names)
                    ),
                );
            }
            if let Some(store) = store
                && !store.keeps(RecordKind::Latest)
            {
                let hint = store.kept_by(RecordKind::Latest).unwrap_or_else(|| {
                    format!(
                        "give a store that keeps what each project last announced, such as \"{}\"",
                        GitBranchKind::NAME
                    )
                });
                self.error(
                    span,
                    &path,
                    format!(
                        "the {} store keeps nothing commands answer with; {hint}",
                        store.kind()
                    ),
                );
            }
        }
    }

    fn destinations(
        &mut self,
        value: Option<Value<'_>>,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> BTreeMap<DestinationName, (Destination, Range<usize>)> {
        let mut destinations = BTreeMap::new();
        for (name, value) in self.named(value, "destinations") {
            let path = format!("destinations.{name}");
            let span = value.span();
            let kinds = self.kinds;
            let names = kinds.destination_kinds();
            let Some((kind, rest)) = self.kinded(value, &path, &names) else {
                continue;
            };
            let destination = kinds.destination_named(&kind).and_then(|kind| {
                kind.parse(
                    self,
                    rest,
                    At {
                        path: &path,
                        span: &span,
                    },
                    read,
                )
            });
            let Some(destination) = destination else {
                continue;
            };
            destinations.insert(DestinationName::new(name), (destination, span));
        }
        destinations
    }

    fn sources(&mut self, value: Option<Value<'_>>) -> BTreeMap<SourceName, Source> {
        let mut sources = BTreeMap::new();
        for (name, value) in self.named(value, "sources") {
            let path = format!("sources.{name}");
            let span = value.span();
            let kinds = self.kinds;
            let Some((kind, rest)) = self.kinded(value, &path, &kinds.source_kinds()) else {
                continue;
            };
            let at = At {
                path: &path,
                span: &span,
            };
            let source = kinds
                .source_named(&kind)
                .and_then(|kind| kind.parse(self, rest, at));
            if let Some(source) = source {
                sources.insert(SourceName::new(name), source);
            }
        }
        sources
    }

    fn templates(
        &mut self,
        value: Option<Value<'_>>,
        read: &dyn Fn(&str) -> Result<String, String>,
    ) -> BTreeMap<TemplateName, Written> {
        let mut templates = BTreeMap::new();
        for (name, value) in self.named(value, "templates") {
            let path = format!("templates.{name}");
            let span = value.span();
            let Some(template) = self.typed::<TemplateFile>(value, &path) else {
                continue;
            };
            let body = match (template.body, template.file) {
                (Some(body), None) => Some((body, format!("{path}.body"))),
                (None, Some(file)) => match read(file.get_ref()) {
                    Ok(body) => Some((Spanned::new(file.span(), body), format!("{path}.file"))),
                    Err(reason) => {
                        self.error(
                            &file.span(),
                            &format!("{path}.file"),
                            format!("{} could not be read: {reason}", file.get_ref()),
                        );
                        None
                    }
                },
                (None, None) => {
                    self.error(&span, &path, "give its body: body = \"…\" or file = \"…\"");
                    None
                }
                (Some(_), Some(_)) => {
                    self.error(&span, &path, "give its body or a file, not both; keep one");
                    None
                }
            };
            if let Some((body, body_path)) = body {
                templates.insert(
                    TemplateName::new(name),
                    Written {
                        title: template.title,
                        body,
                        body_path,
                    },
                );
            }
        }
        templates
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
    let mut loader = Loader {
        text,
        kinds,
        specs: KindSpecs::default(),
        errors: Vec::new(),
    };
    let (root, syntax) = DeTable::parse_recoverable(text);
    for error in syntax {
        loader.error(&error.span().unwrap_or(0..0), "", error.message());
    }
    let sections = loader.sections(root.into_inner());
    loader.version(sections.version);
    loader.specs = loader.declared(sections.kinds);
    let (store, retention) = loader.store(sections.store).unzip();
    let marker_key = sections
        .markers
        .and_then(|value| loader.typed::<MarkersFile>(value, "markers"))
        .map(|markers| markers.key);
    let destinations = loader.destinations(sections.destinations, read);
    let sources = loader.sources(sections.sources);
    let templates = loader.templates(sections.templates, read);
    let listed = loader.apps(sections.apps, &sources);
    let mut routes = Vec::new();
    let mut ids: BTreeSet<RouteId> = BTreeSet::new();
    for (index, value) in loader.routes(sections.routes).into_iter().enumerate() {
        if let Some(read) = loader.route(index, value, &destinations, &sources, &listed, &templates)
        {
            if !ids.insert(read.config.route.id.clone()) {
                loader.error(
                    &read.id_span,
                    &format!("{}.id", read.path),
                    format!(
                        "another route has the id \"{}\"; give each route its own",
                        read.config.route.id.as_str()
                    ),
                );
                continue;
            }
            routes.push(read);
        }
    }
    loader.followed(store.as_ref(), &destinations, &routes);
    loader.answered(store.as_ref(), &destinations, &templates);
    let render = loader.render(sections.render);
    let compiled = loader.compile(&templates, &routes, render);
    if let Some(compiled) = &compiled {
        for read in &routes {
            loader.dry_render(compiled, read);
        }
    }
    let apps = loader.app_routes(listed, &routes);
    let schedule = sections
        .schedule
        .and_then(|value| loader.typed::<ScheduleFile>(value, "schedule"))
        .unwrap_or_default();
    let server = sections
        .server
        .and_then(|value| loader.typed::<ServerFile>(value, "server"))
        .map(|file| loader.server(file, store.as_ref()))
        .unwrap_or_default();
    let http = sections
        .http
        .and_then(|value| loader.typed::<HttpFile>(value, "http"))
        .map(|file| loader.http(file, read))
        .unwrap_or_default();
    loader.outlasted(&destinations, &http);
    loader.waited(&destinations);
    let announce = loader.announce(sections.announce);
    if let (true, Some(store), Some(templates)) = (loader.errors.is_empty(), store, compiled) {
        return Ok(Config {
            store,
            retention: retention.unwrap_or_default(),
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
            schedule: Schedule {
                poll: schedule.poll.map(|poll| poll.0),
                housekeep: schedule.housekeep.map(|housekeep| housekeep.0),
            },
            server,
            http,
            announce,
            kinds: loader.specs,
        });
    }
    let mut errors = loader.errors;
    errors.sort_by_key(|error| (error.line, error.column));
    Err(Errors {
        file: file.to_owned(),
        errors,
    })
}

/// `options`, a store's table written at `span`, apart from how long it
/// keeps what herald records.
fn kept(span: Range<usize>, mut options: DeTable<'_>) -> (Value<'_>, Value<'_>) {
    let mut keep = DeTable::new();
    for key in KeepFile::KEYS {
        if let Some((name, value)) = options.remove_entry(key) {
            keep.insert(name, value);
        }
    }
    (
        Spanned::new(span.clone(), DeValue::Table(options)),
        Spanned::new(span, DeValue::Table(keep)),
    )
}

mod apps;
mod declared;
mod route;
mod settings;
mod templates;
