use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use herald_core::{DestinationName, RouteId, SourceName, TemplateName};
use herald_sources_forge::Forge;
use serde::de::DeserializeOwned;
use toml::Spanned;
use toml::de::{DeTable, DeValue, ValueDeserializer};

use crate::config::{App, Config, Destination, Schedule, Source, Store};
use crate::error::{ConfigError, Errors};
use crate::file::{AppFile, MarkersFile, NoOptions, ScheduleFile, TemplateFile};
use crate::position::position;

type Value<'i> = Spanned<DeValue<'i>>;

/// The version of the config this herald reads.
const VERSION: i64 = 1;
/// The top-level keys of `herald.toml`.
const SECTIONS: [&str; 9] = [
    "version",
    "store",
    "markers",
    "destinations",
    "sources",
    "routes",
    "templates",
    "apps",
    "schedule",
];
const DESTINATION_KINDS: [&str; 1] = ["discord"];
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

struct Loader<'t> {
    text: &'t str,
    errors: Vec<ConfigError>,
}

/// The sections of the file, before each is read.
#[derive(Default)]
struct Sections<'i> {
    version: Option<Value<'i>>,
    store: Option<Value<'i>>,
    markers: Option<Value<'i>>,
    destinations: Option<Value<'i>>,
    sources: Option<Value<'i>>,
    routes: Option<Value<'i>>,
    templates: Option<Value<'i>>,
    apps: Option<Value<'i>>,
    schedule: Option<Value<'i>>,
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

    fn typed<T: DeserializeOwned>(&mut self, value: Value<'_>, path: &str) -> Option<T> {
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
                Some((name.into_owned(), Spanned::new(span, DeValue::Table(table))))
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
                "store" => &mut sections.store,
                "markers" => &mut sections.markers,
                "destinations" => &mut sections.destinations,
                "sources" => &mut sections.sources,
                "routes" => &mut sections.routes,
                "templates" => &mut sections.templates,
                "apps" => &mut sections.apps,
                "schedule" => &mut sections.schedule,
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

    fn store(&mut self, value: Option<Value<'_>>) -> Option<Store> {
        let Some(value) = value else {
            self.error(
                &(0..0),
                "store",
                "choose a store: [store] with kind = \"git-branch\" on GitHub Actions",
            );
            return None;
        };
        let (kind, rest) = self.kinded(value, "store", &Store::KINDS)?;
        if kind == "git-branch" {
            return self
                .typed::<herald_store_git_branch::StoreOptions>(rest, "store")
                .map(Store::GitBranch);
        }
        if kind == "actions-cache" {
            return self
                .typed::<herald_store_actions_cache::StoreOptions>(rest, "store")
                .map(Store::ActionsCache);
        }
        if kind == "repo-variables" {
            return self
                .typed::<herald_store_repo_variables::StoreOptions>(rest, "store")
                .map(Store::RepoVariables);
        }
        self.typed::<NoOptions>(rest, "store")?;
        [
            Store::Sqlite,
            Store::Postgres,
            Store::D1,
            Store::Dynamodb,
            Store::DenoKv,
            Store::Markers,
        ]
        .into_iter()
        .find(|store| store.kind() == kind)
    }

    fn destinations(
        &mut self,
        value: Option<Value<'_>>,
    ) -> BTreeMap<DestinationName, (Destination, Range<usize>)> {
        let mut destinations = BTreeMap::new();
        for (name, value) in self.named(value, "destinations") {
            let path = format!("destinations.{name}");
            let span = value.span();
            let Some((_, rest)) = self.kinded(value, &path, &DESTINATION_KINDS) else {
                continue;
            };
            let Some(options) = self.typed::<herald_dest_discord::DestinationOptions>(rest, &path)
            else {
                continue;
            };
            if let Err(message) = options.bot() {
                self.error(&span, &path, message);
                continue;
            }
            destinations.insert(
                DestinationName::new(name),
                (Destination::Discord(options), span),
            );
        }
        destinations
    }

    fn sources(&mut self, value: Option<Value<'_>>) -> BTreeMap<SourceName, Source> {
        let mut sources = BTreeMap::new();
        for (name, value) in self.named(value, "sources") {
            let path = format!("sources.{name}");
            let Some((kind, rest)) = self.kinded(value, &path, &Source::KINDS) else {
                continue;
            };
            let forge = match kind.as_str() {
                "gitlab" => Forge::GitLab,
                "forgejo" => Forge::Forgejo,
                _ => Forge::GitHub,
            };
            if let Some(options) = self.typed::<herald_sources_forge::SourceOptions>(rest, &path) {
                sources.insert(SourceName::new(name), Source::Forge(forge, options));
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

impl Loader<'_> {
    fn apps(
        &mut self,
        value: Option<Value<'_>>,
        routes: &BTreeSet<RouteId>,
    ) -> BTreeMap<String, App> {
        let mut apps = BTreeMap::new();
        for (name, value) in self.named(value, "apps") {
            let path = format!("apps.{name}");
            let Some(app) = self.typed::<AppFile>(value, &path) else {
                continue;
            };
            let hash = app.token_sha256.get_ref();
            let hex = hash.len() == HASH_DIGITS
                && hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
            if !hex {
                self.error(
                    &app.token_sha256.span(),
                    &format!("{path}.token_sha256"),
                    "a token's SHA-256 is 64 lowercase hex digits, as herald token new prints it",
                );
                continue;
            }
            let mut sound = true;
            for route in &app.routes {
                if !routes.contains(&RouteId::new(route.get_ref())) {
                    self.error(
                        &route.span(),
                        &format!("{path}.routes"),
                        format!("no route has the id \"{}\"", route.get_ref()),
                    );
                    sound = false;
                }
            }
            if sound {
                apps.insert(
                    name,
                    App {
                        token_sha256: hash.clone(),
                        projects: app.projects,
                        routes: app
                            .routes
                            .iter()
                            .map(|route| RouteId::new(route.get_ref()))
                            .collect(),
                    },
                );
            }
        }
        apps
    }
}

/// The config `text` holds, read from the file named `file`, with each
/// template file read through `read`.
///
/// # Errors
///
/// Every mistake in the file, each with its line, column, key and fix.
pub fn load(
    file: &str,
    text: &str,
    read: &dyn Fn(&str) -> Result<String, String>,
) -> Result<Config, Errors> {
    let mut loader = Loader {
        text,
        errors: Vec::new(),
    };
    let (root, syntax) = DeTable::parse_recoverable(text);
    for error in syntax {
        loader.error(&error.span().unwrap_or(0..0), "", error.message());
    }
    let sections = loader.sections(root.into_inner());
    loader.version(sections.version);
    let store = loader.store(sections.store);
    let marker_key = sections
        .markers
        .and_then(|value| loader.typed::<MarkersFile>(value, "markers"))
        .map(|markers| markers.key);
    let destinations = loader.destinations(sections.destinations);
    let sources = loader.sources(sections.sources);
    let templates = loader.templates(sections.templates, read);
    let mut routes = Vec::new();
    let mut ids: BTreeSet<RouteId> = BTreeSet::new();
    let mut values = Vec::new();
    if let Some(value) = sections.routes {
        let span = value.span();
        if let DeValue::Array(items) = value.into_inner() {
            values.extend(items);
        } else {
            loader.error(
                &span,
                "routes",
                "routes are an array of tables; write each as [[routes]]",
            );
        }
    }
    for (index, value) in values.into_iter().enumerate() {
        if let Some(read) = loader.route(index, value, &destinations, &sources, &templates) {
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
    let compiled = loader.compile(&templates, &routes);
    if let Some(compiled) = &compiled {
        for read in &routes {
            loader.dry_render(compiled, read);
        }
    }
    let apps = loader.apps(sections.apps, &ids);
    let schedule = sections
        .schedule
        .and_then(|value| loader.typed::<ScheduleFile>(value, "schedule"))
        .unwrap_or_default();
    if let (true, Some(store), Some(templates)) = (loader.errors.is_empty(), store, compiled) {
        return Ok(Config {
            store,
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
        });
    }
    let mut errors = loader.errors;
    errors.sort_by_key(|error| (error.line, error.column));
    Err(Errors {
        file: file.to_owned(),
        errors,
    })
}

mod route;
mod templates;
