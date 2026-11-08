use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use herald_core::{
    DestinationName, Inbound, RecordKind, Retention, RouteId, Sandbox, SourceName, TemplateName,
};
use herald_sources_forge::Forge;
use herald_sources_registry::Registry;
use serde::de::DeserializeOwned;
use toml::Spanned;
use toml::de::{DeTable, DeValue, ValueDeserializer};

use crate::config::{Config, Destination, KeyCache, Schedule, Server, Source, Store};
use crate::error::{ConfigError, Errors};
use crate::file::{
    HttpFile, InboundFile, Keep, KeepFile, KeyCacheFile, MarkersFile, NoOptions, RenderFile,
    ScheduleFile, ServerFile, TemplateFile,
};
use crate::position::position;
use herald_ports::{Outbound, Proxy, Roots};

type Value<'i> = Spanned<DeValue<'i>>;

/// The version of the config this herald reads.
const VERSION: i64 = 1;
/// The top-level keys of `herald.toml`.
const SECTIONS: [&str; 12] = [
    "version",
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
    server: Option<Value<'i>>,
    http: Option<Value<'i>>,
    render: Option<Value<'i>>,
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
                "server" => &mut sections.server,
                "http" => &mut sections.http,
                "render" => &mut sections.render,
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
        let (kind, rest) = self.kinded(value, "store", &Store::KINDS)?;
        if kind == Store::MARKERS {
            self.typed::<NoOptions>(rest, "store")?;
            return Some((Store::Markers, Retention::default()));
        }
        let (rest, keep) = kept(rest);
        let retention = self.retention(keep);
        let store = self.options(&kind, rest)?;
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

    fn options(&mut self, kind: &str, rest: Value<'_>) -> Option<Store> {
        if kind == Store::SQLITE {
            return self
                .typed::<herald_store_sqlite::StoreOptions>(rest, "store")
                .map(Store::Sqlite);
        }
        if kind == Store::POSTGRES {
            return self
                .typed::<herald_store_postgres::StoreOptions>(rest, "store")
                .map(Store::Postgres);
        }
        if kind == Store::GIT_BRANCH {
            return self
                .typed::<herald_store_git_branch::StoreOptions>(rest, "store")
                .map(Store::GitBranch);
        }
        if kind == Store::ACTIONS_CACHE {
            return self
                .typed::<herald_store_actions_cache::StoreOptions>(rest, "store")
                .map(Store::ActionsCache);
        }
        if kind == Store::REPO_VARIABLES {
            return self
                .typed::<herald_store_repo_variables::StoreOptions>(rest, "store")
                .map(Store::RepoVariables);
        }
        if kind == Store::CLOUDFLARE_D1 {
            return self
                .typed::<herald_store_d1::StoreOptions>(rest, "store")
                .map(Store::D1);
        }
        if kind == Store::DURABLE_OBJECT {
            return self
                .typed::<herald_store_durable_object::StoreOptions>(rest, "store")
                .map(Store::DurableObject);
        }
        if kind == Store::DYNAMODB {
            return self
                .typed::<herald_store_dynamodb::StoreOptions>(rest, "store")
                .map(Store::Dynamodb);
        }
        if kind == Store::DENO_KV {
            return self
                .typed::<herald_store_deno_kv::StoreOptions>(rest, "store")
                .map(Store::DenoKv);
        }
        self.typed::<herald_store_fastly_kv::StoreOptions>(rest, "store")
            .map(Store::FastlyKv)
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
                    Store::GIT_BRANCH
                )
            })
        };
        for read in routes {
            let route = &read.config.route;
            let follows = matches!(
                destinations.get(&route.destination),
                Some((Destination::ActivityPub(_), _))
            );
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
                        Store::GIT_BRANCH
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
    ) -> BTreeMap<DestinationName, (Destination, Range<usize>)> {
        let mut destinations = BTreeMap::new();
        for (name, value) in self.named(value, "destinations") {
            let path = format!("destinations.{name}");
            let span = value.span();
            let Some((kind, rest)) = self.kinded(value, &path, &Destination::KINDS) else {
                continue;
            };
            let destination = if kind == Destination::MASTODON {
                self.typed::<herald_dest_fediverse::DestinationOptions>(rest, &path)
                    .map(Destination::Mastodon)
            } else if kind == Destination::MATRIX {
                self.matrix(rest, &path, &span)
            } else if kind == Destination::ACTIVITYPUB {
                self.actor(rest, &path, &span)
            } else if kind == Destination::SLACK {
                self.slack(rest, &path, &span)
            } else {
                self.discord(rest, &path, &span)
            };
            let Some(destination) = destination else {
                continue;
            };
            destinations.insert(DestinationName::new(name), (destination, span));
        }
        destinations
    }

    fn matrix(&mut self, rest: Value<'_>, path: &str, span: &Range<usize>) -> Option<Destination> {
        let options = self.typed::<herald_dest_matrix::DestinationOptions>(rest, path)?;
        if let Some(interactions) = &options.interactions {
            self.interactions(span, path, interactions.check());
        }
        Some(Destination::Matrix(options))
    }

    fn actor(&mut self, rest: Value<'_>, path: &str, span: &Range<usize>) -> Option<Destination> {
        let options = self.typed::<herald_dest_fediverse::ActorOptions>(rest, path)?;
        if let Err(message) = options.check() {
            self.error(span, path, message);
            return None;
        }
        Some(Destination::ActivityPub(options))
    }

    fn slack(&mut self, rest: Value<'_>, path: &str, span: &Range<usize>) -> Option<Destination> {
        let options = self.typed::<herald_dest_slack::DestinationOptions>(rest, path)?;
        if let Err(message) = options.bot() {
            self.error(span, path, message);
            return None;
        }
        if let Some(interactions) = &options.interactions {
            self.interactions(span, path, interactions.check());
        }
        Some(Destination::Slack(options))
    }

    fn discord(&mut self, rest: Value<'_>, path: &str, span: &Range<usize>) -> Option<Destination> {
        let options = self.typed::<herald_dest_discord::DestinationOptions>(rest, path)?;
        let bot = match options.bot() {
            Ok(bot) => bot,
            Err(message) => {
                self.error(span, path, message);
                return None;
            }
        };
        if let Some(interactions) = &options.interactions {
            let checked = interactions.check().and_then(|()| {
                if interactions.via == herald_dest_discord::Via::Gateway && !bot {
                    Err("the gateway is joined as a bot; give the destination a token in place of its webhook".to_owned())
                } else {
                    Ok(())
                }
            });
            self.interactions(span, path, checked);
        }
        Some(Destination::Discord(options))
    }

    fn interactions(&mut self, span: &Range<usize>, path: &str, checked: Result<(), String>) {
        if let Err(message) = checked {
            self.error(span, &format!("{path}.interactions"), message);
        }
    }

    fn sources(&mut self, value: Option<Value<'_>>) -> BTreeMap<SourceName, Source> {
        let mut sources = BTreeMap::new();
        for (name, value) in self.named(value, "sources") {
            let path = format!("sources.{name}");
            let span = value.span();
            let Some((kind, rest)) = self.kinded(value, &path, &Source::KINDS) else {
                continue;
            };
            let source = if kind == Source::FEED {
                self.typed::<herald_sources_feed::FeedOptions>(rest, &path)
                    .and_then(|options| self.sound(&span, &path, options.check(), options))
                    .map(Source::Feed)
            } else if let Some(forge) = Forge::ALL.into_iter().find(|forge| forge.kind() == kind) {
                self.typed::<herald_sources_forge::SourceOptions>(rest, &path)
                    .and_then(|options| self.sound(&span, &path, options.check(forge), options))
                    .map(|options| Source::Forge(forge, options))
            } else {
                let registry = Registry::ALL
                    .into_iter()
                    .find(|registry| registry.kind() == kind)
                    .unwrap_or(Registry::Packagist);
                let options = self.typed::<herald_sources_registry::RegistryOptions>(rest, &path);
                match options {
                    Some(options) if options.token.is_some() && !registry.takes_token() => {
                        self.error(
                            &span,
                            &format!("{path}.token"),
                            format!("the registry \"{kind}\" takes no token; remove token"),
                        );
                        None
                    }
                    options => options
                        .and_then(|options| {
                            self.sound(&span, &path, options.check(registry), options)
                        })
                        .map(|options| Source::Registry(registry, options)),
                }
            };
            if let Some(source) = source {
                sources.insert(SourceName::new(name), source);
            }
        }
        sources
    }

    /// `options`, where `checked` found them sound, and otherwise nothing,
    /// with the reason recorded at `path`.
    fn sound<T>(
        &mut self,
        span: &Range<usize>,
        path: &str,
        checked: Result<(), String>,
        options: T,
    ) -> Option<T> {
        match checked {
            Ok(()) => Some(options),
            Err(message) => {
                self.error(span, path, message);
                None
            }
        }
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
    /// `[http]` as `file` gives it, its CA file read through `read`.
    fn http(&mut self, file: HttpFile, read: &dyn Fn(&str) -> Result<String, String>) -> Outbound {
        let defaults = Outbound::default();
        let total = file.total.map_or(defaults.total, |total| total.0);
        let connect = file.connect.map_or(defaults.connect, |connect| {
            if connect.get_ref().0 > total {
                self.error(
                    &connect.span(),
                    "http.connect",
                    "a connection may take no longer than the whole request; make connect at most total",
                );
            }
            connect.into_inner().0
        });
        let proxy = file.proxy.map_or(Proxy::Environment, |proxy| {
            match proxy.get_ref().as_str() {
                "environment" => Proxy::Environment,
                "none" => Proxy::None,
                url if url.starts_with("http://") || url.starts_with("https://") => {
                    Proxy::At(url.to_owned())
                }
                other => {
                    self.error(
                        &proxy.span(),
                        "http.proxy",
                        format!(
                            "\"{other}\" is no proxy; write \"environment\", \"none\" or a proxy's http or https URL"
                        ),
                    );
                    Proxy::None
                }
            }
        });
        let roots = file
            .ca_file
            .and_then(|ca_file| match read(ca_file.get_ref()) {
                Ok(pem) if pem.contains("-----BEGIN CERTIFICATE-----") => Some(Roots {
                    file: ca_file.get_ref().clone(),
                    pem,
                }),
                Ok(_) => {
                    self.error(
                        &ca_file.span(),
                        "http.ca_file",
                        format!("{} holds no PEM certificate", ca_file.get_ref()),
                    );
                    None
                }
                Err(error) => {
                    self.error(&ca_file.span(), "http.ca_file", error);
                    None
                }
            });
        let body = self.most(
            file.body.map(|body| (body.span(), body.into_inner().0)),
            defaults.body,
            Outbound::BODY_MOST,
            "http.body",
            &size(Outbound::BODY_MOST),
        );
        Outbound {
            connect,
            total,
            proxy,
            roots,
            body,
        }
    }

    /// `[server]` as `file` gives it, over the store `store`.
    fn server(&mut self, file: ServerFile, store: Option<&Store>) -> Server {
        let defaults = Server::default();
        let listen = file.listen.map_or(defaults.listen, |listen| {
            if listen.get_ref().parse::<std::net::SocketAddr>().is_err() {
                self.error(
                    &listen.span(),
                    "server.listen",
                    format!(
                        "\"{}\" is not an address and a port; write one such as \"{}\"",
                        listen.get_ref(),
                        Server::LISTEN
                    ),
                );
            }
            listen.into_inner()
        });
        let keys = file.keys.cache.map_or(KeyCache::Memory, |cache| {
            if *cache.get_ref() == KeyCacheFile::Memory {
                return KeyCache::Memory;
            }
            if let Some(store) = store
                && !store.keeps(RecordKind::Keys)
            {
                let hint = store
                    .kept_by(RecordKind::Keys)
                    .unwrap_or_else(|| "give a store that keeps records".to_owned());
                self.error(
                    &cache.span(),
                    "server.keys.cache",
                    format!(
                        "the {} store keeps no keys; keep them in \"memory\", or {hint}",
                        store.kind()
                    ),
                );
            }
            KeyCache::Store
        });
        let count = |given: Option<std::num::NonZeroUsize>, default| {
            given.map_or(default, std::num::NonZeroUsize::get)
        };
        Server {
            listen,
            jobs: count(file.jobs, defaults.jobs),
            connections: count(file.connections, defaults.connections),
            body: self.most(
                file.body.map(|body| (body.span(), body.into_inner().0)),
                defaults.body,
                Server::BODY_MOST,
                "server.body",
                &size(Server::BODY_MOST),
            ),
            fetches: count(file.fetches, defaults.fetches),
            completions: count(file.completions, defaults.completions),
            deliveries: count(file.deliveries, defaults.deliveries),
            silence: file.silence.map_or(defaults.silence, |silence| silence.0),
            keys,
            keys_refreshed: file
                .keys
                .refresh_after
                .map_or(defaults.keys_refreshed, |after| after.0),
            keys_reread: file
                .keys
                .reread_after
                .map_or(defaults.keys_reread, |after| after.0),
            keys_most: count(file.keys.most, defaults.keys_most),
            inbound: self.inbound(file.inbound),
        }
    }

    /// `given`, where it is no more than `most`; a value past it is named
    /// as a mistake by `path`, with `most` written as `shown`.
    fn most<T: PartialOrd + Copy>(
        &mut self,
        given: Option<(Range<usize>, T)>,
        default: T,
        most: T,
        path: &str,
        shown: &str,
    ) -> T {
        let Some((span, given)) = given else {
            return default;
        };
        if given > most {
            self.error(
                &span,
                path,
                format!(
                    "make it at most {shown}, four times its default; a wider bound loosens what herald holds a request or a template to"
                ),
            );
        }
        given
    }

    fn inbound(&mut self, file: InboundFile) -> Inbound {
        let defaults = Inbound::DEFAULT;
        let most = Inbound::MOST;
        let covered = file
            .covered
            .map(|covered| (covered.span(), covered.into_inner().get()));
        if let Some((span, given)) = &covered
            && *given < Inbound::LEAST_COVERED
        {
            self.error(
                span,
                "server.inbound.covered",
                format!(
                    "an ActivityPub signature covers at least {} headers; make it at least that",
                    Inbound::LEAST_COVERED
                ),
            );
        }
        let covered = self.most(
            covered,
            defaults.covered,
            most.covered,
            "server.inbound.covered",
            &most.covered.to_string(),
        );
        Inbound {
            replay_window: self.most(
                file.replay_window
                    .map(|window| (window.span(), window.into_inner().0)),
                defaults.replay_window,
                most.replay_window,
                "server.inbound.replay_window",
                &format!("\"{}m\"", most.replay_window.as_mins()),
            ),
            signatures: self.most(
                file.signatures
                    .map(|signatures| (signatures.span(), signatures.into_inner().get())),
                defaults.signatures,
                most.signatures,
                "server.inbound.signatures",
                &most.signatures.to_string(),
            ),
            covered,
        }
    }

    fn sandbox(&mut self, file: RenderFile) -> Sandbox {
        let defaults = Sandbox::DEFAULT;
        let most = Sandbox::MOST;
        Sandbox {
            fuel: self.most(
                file.fuel.map(|fuel| (fuel.span(), fuel.into_inner().get())),
                defaults.fuel,
                most.fuel,
                "render.fuel",
                &most.fuel.to_string(),
            ),
            recursion: self.most(
                file.recursion
                    .map(|recursion| (recursion.span(), recursion.into_inner().get())),
                defaults.recursion,
                most.recursion,
                "render.recursion",
                &most.recursion.to_string(),
            ),
            output: self.most(
                file.output
                    .map(|output| (output.span(), output.into_inner().0)),
                defaults.output,
                most.output,
                "render.output",
                &size(most.output),
            ),
        }
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
    let (store, retention) = loader.store(sections.store).unzip();
    let marker_key = sections
        .markers
        .and_then(|value| loader.typed::<MarkersFile>(value, "markers"))
        .map(|markers| markers.key);
    let destinations = loader.destinations(sections.destinations);
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
    let render = sections
        .render
        .and_then(|value| loader.typed::<RenderFile>(value, "render"))
        .map_or(Sandbox::DEFAULT, |file| loader.sandbox(file));
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
        });
    }
    let mut errors = loader.errors;
    errors.sort_by_key(|error| (error.line, error.column));
    Err(Errors {
        file: file.to_owned(),
        errors,
    })
}

/// `bytes` as a config writes a size, in quotes.
fn size(bytes: usize) -> String {
    use crate::file::{KIB, MIB};
    if bytes.is_multiple_of(MIB) {
        format!("\"{}MiB\"", bytes / MIB)
    } else {
        format!("\"{}KiB\"", bytes / KIB)
    }
}

/// `rest`, a store's options, apart from how long it keeps what herald
/// records.
fn kept(rest: Value<'_>) -> (Value<'_>, Value<'_>) {
    let span = rest.span();
    let mut keep = DeTable::new();
    let options = match rest.into_inner() {
        DeValue::Table(mut table) => {
            for key in KeepFile::KEYS {
                if let Some((name, value)) = table.remove_entry(key) {
                    keep.insert(name, value);
                }
            }
            DeValue::Table(table)
        }
        other => other,
    };
    (
        Spanned::new(span.clone(), options),
        Spanned::new(span, DeValue::Table(keep)),
    )
}

mod apps;
mod route;
mod templates;
