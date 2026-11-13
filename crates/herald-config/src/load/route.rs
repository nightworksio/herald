//! `[[routes]]`: each route read against the destinations, sources, apps
//! and templates it names, and checked against what its destination can do.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use herald_core::{
    Delivery, DestinationName, Filter, Kind, Mentions, Pattern, Profile, RoleId, Route, RouteId,
    SourceName, TemplateName, Thread,
};
use toml::Spanned;
use toml::de::{DeTable, DeValue};

use super::apps::Named;
use super::templates::Written;
use super::{Loader, Value, list};
use herald_kinds::{Destination, Placed, Placing, Source};

use crate::config::RouteConfig;
use crate::file::{ROUTE_KEYS, RouteFile, ThreadFile};
use crate::kinds::At;

/// A route as its keys say, before its templates are compiled.
pub(super) struct Read {
    pub(super) config: RouteConfig,
    pub(super) title_span: Range<usize>,
    pub(super) body_span: Range<usize>,
    pub(super) body_path: String,
    pub(super) sample_kinds: BTreeSet<Kind>,
    pub(super) thread: Option<Spanned<String>>,
    pub(super) id_span: Range<usize>,
    pub(super) path: String,
}

/// What a route takes events by: its sources, kinds and versions.
#[derive(Clone, Copy)]
struct Taken<'r> {
    sources: &'r [Spanned<String>],
    kinds: &'r [Spanned<Kind>],
    versions: Option<&'r Spanned<String>>,
}

/// How a route reaches its destination: by what name, able to do what, what
/// a bot could do there, and whether each post opens a thread of its own.
#[derive(Clone, Copy)]
struct Reach<'r> {
    name: &'static str,
    profile: &'r Profile,
    as_bot: Option<&'r Profile>,
    threads_each_post: bool,
}

impl Loader<'_> {
    /// The kinds of event `route` takes, each known, where its versions
    /// can match them.
    fn sample_kinds(
        &mut self,
        route: Taken<'_>,
        path: &str,
        sources: &BTreeMap<SourceName, Source>,
        apps: &BTreeMap<String, Named>,
    ) -> Option<BTreeSet<Kind>> {
        let mut sound = true;
        for (index, kind) in route.kinds.iter().enumerate() {
            if self.specs.get(kind.get_ref()).is_none() {
                let names: Vec<String> = self
                    .specs
                    .iter()
                    .map(|spec| spec.kind.to_string())
                    .collect();
                let names: Vec<&str> = names.iter().map(String::as_str).collect();
                self.error(
                    &kind.span(),
                    &format!("{path}.kinds[{index}]"),
                    format!(
                        "there is no kind \"{}\"; the kinds are {}, or declare it under [kinds.{0}]",
                        kind.get_ref(),
                        list(&names)
                    ),
                );
                sound = false;
            }
        }
        if !sound {
            return None;
        }
        let kinds: BTreeSet<Kind> = if route.kinds.is_empty() {
            route
                .sources
                .iter()
                .flat_map(|source| {
                    let name = source.get_ref();
                    let posted: Vec<Kind> = if apps.contains_key(name) {
                        self.specs.iter().map(|spec| spec.kind.clone()).collect()
                    } else {
                        Vec::new()
                    };
                    sources
                        .get(&SourceName::new(name))
                        .map(|source| source.kinds().to_vec())
                        .unwrap_or_default()
                        .into_iter()
                        .chain(posted)
                })
                .collect()
        } else {
            route
                .kinds
                .iter()
                .map(|kind| kind.get_ref().clone())
                .collect()
        };
        let unversioned: Vec<&str> = kinds
            .iter()
            .filter(|kind| !self.specs.get(kind).is_some_and(|spec| spec.versioned))
            .map(Kind::as_str)
            .collect();
        if let (Some(versions), false) = (route.versions, unversioned.is_empty()) {
            self.error(
                &versions.span(),
                &format!("{path}.versions"),
                format!(
                    "{} events carry no version, so a route with versions takes none of them; give kinds that are versioned, such as kinds = [\"release\", \"package-version\"], or remove versions",
                    list(&unversioned)
                ),
            );
            return None;
        }
        Some(kinds)
    }

    fn split<'i>(&mut self, value: Value<'i>, path: &str) -> Option<(Value<'i>, Value<'i>)> {
        let (span, table) = self.table(value, path)?;
        let (common, rest): (DeTable<'i>, DeTable<'i>) = table
            .into_iter()
            .partition(|(key, _)| ROUTE_KEYS.contains(&key.get_ref().as_ref()));
        Some((
            Spanned::new(span.clone(), DeValue::Table(common)),
            Spanned::new(span, DeValue::Table(rest)),
        ))
    }

    fn sources_named(
        &mut self,
        route: &RouteFile,
        path: &str,
        sources: &BTreeMap<SourceName, Source>,
        apps: &BTreeMap<String, Named>,
    ) -> bool {
        let mut sound = true;
        for source in &route.sources {
            let name = source.get_ref();
            if !sources.contains_key(&SourceName::new(name)) && !apps.contains_key(name) {
                let mut names: Vec<&str> = sources
                    .keys()
                    .map(SourceName::as_str)
                    .chain(apps.keys().map(String::as_str))
                    .collect();
                names.sort_unstable();
                let hint = if names.is_empty() {
                    format!("name it under [sources.{name}] or [apps.{name}]")
                } else {
                    format!("the sources and apps are {}", list(&names))
                };
                self.error(
                    &source.span(),
                    &format!("{path}.sources"),
                    format!("no source or app is named \"{name}\"; {hint}"),
                );
                sound = false;
            }
        }
        sound
    }

    fn placed(
        &mut self,
        reached: &Destination,
        rest: Value<'_>,
        route: &RouteFile,
        path: &str,
        destination: &DestinationName,
        sound: &mut bool,
    ) -> Option<(Placed, &'static str)> {
        let span = route.id.span();
        let kind = self.kinds.destination_named(reached.kind())?;
        let placing = Placing {
            destination: destination.as_str(),
            subscribers: route.mentions.subscribers,
        };
        let (placed, name, clean) =
            kind.place(self, reached, rest, At { path, span: &span }, placing)?;
        *sound &= clean;
        Some((placed, name))
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

    /// Each route of `[[routes]]`, its id one no other route has.
    pub(super) fn every_route(
        &mut self,
        value: Option<Value<'_>>,
        destinations: &BTreeMap<DestinationName, (Destination, Range<usize>)>,
        sources: &BTreeMap<SourceName, Source>,
        apps: &BTreeMap<String, Named>,
        templates: &BTreeMap<TemplateName, Written>,
    ) -> Vec<Read> {
        let mut routes = Vec::new();
        let mut ids: BTreeSet<RouteId> = BTreeSet::new();
        for (index, value) in self.routes(value).into_iter().enumerate() {
            let Some(read) = self.route(index, value, destinations, sources, apps, templates)
            else {
                continue;
            };
            if ids.insert(read.config.route.id.clone()) {
                routes.push(read);
            } else {
                self.error(
                    &read.id_span,
                    &format!("{}.id", read.path),
                    format!(
                        "another route has the id \"{}\"; give each route its own",
                        read.config.route.id.as_str()
                    ),
                );
            }
        }
        routes
    }

    /// The destination `route` names as `destination`, one of
    /// `destinations`.
    fn reached<'d>(
        &mut self,
        destination: &DestinationName,
        route: &RouteFile,
        path: &str,
        destinations: &'d BTreeMap<DestinationName, (Destination, Range<usize>)>,
    ) -> Option<&'d Destination> {
        if let Some((reached, _)) = destinations.get(destination) {
            return Some(reached);
        }
        let names: Vec<&str> = destinations.keys().map(DestinationName::as_str).collect();
        self.error(
            &route.destination.span(),
            &format!("{path}.destination"),
            format!(
                "no destination is named \"{}\"; the destinations are {}",
                route.destination.get_ref(),
                list(&names)
            ),
        );
        None
    }

    /// The template `route` names as `template`, where `templates` has it.
    fn written<'t>(
        &mut self,
        template: &TemplateName,
        route: &RouteFile,
        path: &str,
        templates: &'t BTreeMap<TemplateName, Written>,
    ) -> Option<&'t Written> {
        let written = templates.get(template);
        if written.is_none() {
            let names: Vec<&str> = templates.keys().map(TemplateName::as_str).collect();
            self.error(
                &route.template.span(),
                &format!("{path}.template"),
                format!(
                    "no template is named \"{}\"; the templates are {}",
                    route.template.get_ref(),
                    list(&names)
                ),
            );
        }
        written
    }

    /// Whether the destination `destination`, as `reach` reaches it, can do
    /// everything `delivery` asks, each thing it cannot said at the route
    /// written at `path` and `span`.
    fn deliverable(
        &mut self,
        path: &str,
        span: &Range<usize>,
        destination: &DestinationName,
        reach: Reach<'_>,
        delivery: &Delivery,
    ) -> bool {
        let mut sound = true;
        let lacking = reach.profile.features.lacking(delivery);
        let bot_lacks = reach.as_bot.map(|bot| bot.features.lacking(delivery));
        for key in lacking
            .into_iter()
            .filter(|key| !(reach.threads_each_post && *key == "thread"))
        {
            let remedy = if bot_lacks
                .as_ref()
                .is_some_and(|lacks| !lacks.contains(&key))
            {
                "or reach it with a bot token".to_owned()
            } else {
                format!("as {} has no way to do it", reach.name)
            };
            self.error(
                span,
                &format!("{path}.{key}"),
                format!(
                    "the destination \"{}\" cannot do this as it is reached; remove {key}, {remedy}",
                    destination.as_str()
                ),
            );
            sound = false;
        }
        sound
    }

    fn route(
        &mut self,
        index: usize,
        value: Value<'_>,
        destinations: &BTreeMap<DestinationName, (Destination, Range<usize>)>,
        sources: &BTreeMap<SourceName, Source>,
        apps: &BTreeMap<String, Named>,
        templates: &BTreeMap<TemplateName, Written>,
    ) -> Option<Read> {
        let path = format!("routes[{index}]");
        let (common, rest) = self.split(value, &path)?;
        let route = self.typed::<RouteFile>(common, &path)?;
        let mut sound = self.sources_named(&route, &path, sources, apps);
        let destination = DestinationName::new(route.destination.get_ref());
        let reached = self.reached(&destination, &route, &path, destinations)?;
        let template = TemplateName::new(route.template.get_ref());
        let written = self.written(&template, &route, &path, templates);
        let span = route.id.span();
        let (options, name) =
            self.placed(reached, rest, &route, &path, &destination, &mut sound)?;
        let (profile, as_bot) = reached.profile(&options)?;
        let profile = Profile {
            symbols: reached.symbols(),
            ..profile
        };
        let thread_name = route.thread.as_ref().map(|thread| thread.name.clone());
        let delivery = Delivery {
            crosspost: route.crosspost,
            thread: route.thread.map(opened),
            pin: route.pin,
            mentions: Mentions {
                roles: mentioned(route.mentions.roles, options.opt_in_role()),
                subscribers: route.mentions.subscribers,
            },
        };
        let reach = Reach {
            name,
            profile: &profile,
            as_bot: as_bot.as_ref(),
            threads_each_post: options.threads_each_post(),
        };
        sound &= self.deliverable(&path, &span, &destination, reach, &delivery);
        let (Some(written), true) = (written, sound) else {
            return None;
        };
        let taken = Taken {
            sources: &route.sources,
            kinds: &route.kinds,
            versions: route.versions.as_ref(),
        };
        let sample_kinds = self.sample_kinds(taken, &path, sources, apps)?;
        let filter = Filter {
            sources: route
                .sources
                .iter()
                .map(|source| SourceName::new(source.get_ref()))
                .collect(),
            kinds: route.kinds.into_iter().map(Spanned::into_inner).collect(),
            prereleases: route.prereleases,
            versions: route
                .versions
                .map(|versions| Pattern::new(versions.into_inner())),
        };
        let config = RouteConfig {
            route: Route {
                id: RouteId::new(route.id.get_ref()),
                filter,
                destination,
                template,
                delivery,
            },
            options,
            profile,
        };
        Some(Read {
            config,
            title_span: written.title.span(),
            body_span: written.body.span(),
            body_path: written.body_path.clone(),
            sample_kinds,
            thread: thread_name,
            id_span: span,
            path,
        })
    }
}

/// The thread `thread` asks a route to open.
fn opened(thread: ThreadFile) -> Thread {
    Thread {
        name: thread.name.into_inner(),
        archive_after: thread.archive_after.map(|after| after.0),
        lock_after: thread.lock_after.map(|after| after.0),
    }
}

/// Each role of `roles`, then the opt-in role `opt_in`, mentioned once.
fn mentioned(roles: Vec<String>, opt_in: Option<String>) -> Vec<RoleId> {
    roles
        .into_iter()
        .chain(opt_in)
        .fold(Vec::new(), |mut roles, role| {
            if !roles.contains(&role) {
                roles.push(role);
            }
            roles
        })
        .into_iter()
        .map(RoleId::new)
        .collect()
}
