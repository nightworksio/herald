//! `[[routes]]`: each route read against the destinations, sources, apps
//! and templates it names, and checked against what its destination can do.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use herald_core::{
    Delivery, DestinationName, Filter, Glob, Kind, Match, Mentions, Pattern, Pinning, Pins,
    Profile, RoleId, Route, RouteId, SourceName, TemplateName, Thread,
};
use toml::Spanned;
use toml::de::DeValue;

use super::apps::Named;
use super::boards::BoardRoute;
use crate::views::{Report, Rule};

/// What a route may name among its sources beside `[sources]`: the apps,
/// the reports and the rules.
#[derive(Clone, Copy)]
pub(super) struct Others<'a> {
    pub(super) apps: &'a BTreeMap<String, Named>,
    pub(super) reports: &'a BTreeMap<String, Report>,
    pub(super) rules: &'a BTreeMap<String, Rule>,
}
use super::templates::Written;
use super::{Loader, Value, list};
use herald_kinds::{Destination, Placed, Placing, Source};

use crate::config::RouteConfig;
use crate::file::{MatchFile, PinFile, ROUTE_KEYS, RouteFile, ThreadFile};
use crate::kinds::At;

/// A route as its keys say, before its templates are compiled.
pub(super) struct Read {
    pub(super) config: RouteConfig,
    pub(super) rendered: Option<Rendered>,
    pub(super) sample_kinds: BTreeSet<Kind>,
    pub(super) origins: bool,
    pub(super) thread: Option<Spanned<String>>,
    pub(super) group: Option<Spanned<String>>,
    pub(super) id_span: Range<usize>,
    pub(super) path: String,
}

/// The template a route renders with, and where its parts are written.
pub(super) struct Rendered {
    pub(super) template: TemplateName,
    pub(super) title_span: Range<usize>,
    pub(super) body_span: Range<usize>,
    pub(super) body_path: String,
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
    /// The match `file` writes at `path`, where each of its globs reads,
    /// each attribute it names is one of a kind in `kinds`, and its `not`
    /// narrows by something and holds no `not` of its own.
    fn matching(
        &mut self,
        mut file: MatchFile,
        path: &str,
        kinds: &BTreeSet<Kind>,
    ) -> Option<Match> {
        let at = format!("{path}.match");
        let not = file.not.take().map(|not| {
            let (span, not) = (not.span(), not.into_inner());
            let refused = if not.not.is_some() {
                Some("a not holds no not of its own; write what it never takes beside it")
            } else if not.is_empty() {
                Some("not narrows by nothing, so it would take every event; name what is never taken, or remove it")
            } else {
                None
            };
            if let Some(refused) = refused {
                self.error(&span, &format!("{at}.not"), refused);
            }
            self.matched(*not, &format!("{at}.not"), kinds)
        });
        let matching = self.matched(file, &at, kinds)?;
        let not = match not {
            Some(not) => Some(Box::new(not?)),
            None => None,
        };
        Some(Match { not, ..matching })
    }

    /// The match `file` writes at `at`, leaving `not` aside.
    fn matched(&mut self, file: MatchFile, at: &str, kinds: &BTreeSet<Kind>) -> Option<Match> {
        let mut sound = true;
        let mut globs = |written: Vec<Spanned<String>>, key: &str| -> Vec<Glob> {
            written
                .into_iter()
                .enumerate()
                .filter_map(|(index, text)| match Glob::new(text.get_ref()) {
                    Ok(glob) => Some(glob),
                    Err(error) => {
                        self.error(
                            &text.span(),
                            &format!("{at}.{key}[{index}]"),
                            error.to_string(),
                        );
                        sound = false;
                        None
                    }
                })
                .collect()
        };
        let projects = globs(file.projects, "projects");
        let repositories = globs(file.repositories, "repositories");
        let owners = globs(file.owners, "owners");
        let attributes = file.attributes.map_or_else(BTreeMap::new, |attributes| {
            let span = attributes.span();
            let attributes = attributes.into_inner();
            if let Some(unknown) = attributes.keys().find(|name| !self.carries(kinds, name)) {
                let names: BTreeSet<&str> = kinds
                    .iter()
                    .filter_map(|kind| self.specs.get(kind))
                    .flat_map(|spec| spec.attributes.keys().map(String::as_str))
                    .collect();
                let names: Vec<&str> = names.into_iter().collect();
                self.error(
                    &span,
                    &format!("{at}.attributes.{unknown}"),
                    format!(
                        "no kind the route takes carries an attribute \"{unknown}\"; its kinds carry {}",
                        list(&names)
                    ),
                );
                sound = false;
            }
            attributes
        });
        sound.then_some(Match {
            projects,
            repositories,
            owners,
            forges: file.forges,
            hosts: file.hosts,
            topics: file.topics,
            properties: file.properties,
            attributes,
            not: None,
        })
    }

    /// Whether some kind of `kinds` carries the attribute `name`.
    fn carries(&self, kinds: &BTreeSet<Kind>, name: &str) -> bool {
        kinds
            .iter()
            .filter_map(|kind| self.specs.get(kind))
            .any(|spec| spec.attributes.contains_key(name))
    }

    /// The private kinds of `taken`, the kinds a route takes.
    fn private_kinds(&self, taken: &BTreeSet<Kind>) -> Vec<Kind> {
        taken
            .iter()
            .filter(|kind| self.specs.get(kind).is_some_and(|spec| spec.private))
            .cloned()
            .collect()
    }

    /// The kinds of event `route` takes, each known, where its versions
    /// can match them.
    fn sample_kinds(
        &mut self,
        route: Taken<'_>,
        path: &str,
        sources: &BTreeMap<SourceName, Source>,
        others: Others<'_>,
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
        let announced: BTreeSet<Kind> = route
            .sources
            .iter()
            .flat_map(|source| {
                let name = source.get_ref();
                let named = sources.get(&SourceName::new(name));
                let posted: Vec<Kind> = if others.reports.contains_key(name) {
                    vec![Kind::REPORT]
                } else if others.rules.contains_key(name) {
                    vec![Kind::NUDGE]
                } else if others.apps.contains_key(name)
                    || named.is_some_and(|source| source.any_kind())
                {
                    self.specs
                        .iter()
                        .map(|spec| spec.kind.clone())
                        .filter(|kind| !kind.own())
                        .collect()
                } else {
                    Vec::new()
                };
                named
                    .map(|source| source.kinds())
                    .unwrap_or_default()
                    .into_iter()
                    .chain(posted)
            })
            .collect();
        let kinds: BTreeSet<Kind> = if route.kinds.is_empty() {
            announced
        } else {
            for (index, kind) in route.kinds.iter().enumerate() {
                if !announced.contains(kind.get_ref()) {
                    let names: Vec<&str> = route
                        .sources
                        .iter()
                        .map(|source| source.get_ref().as_str())
                        .collect();
                    self.error(
                        &kind.span(),
                        &format!("{path}.kinds[{index}]"),
                        format!(
                            "no source the route names announces a \"{}\" event, so the route takes none; have {} announce it, such as through its signals, or remove it",
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

    fn sources_named(
        &mut self,
        route: &RouteFile,
        path: &str,
        sources: &BTreeMap<SourceName, Source>,
        others: Others<'_>,
    ) -> bool {
        let mut sound = true;
        for source in &route.sources {
            let name = source.get_ref();
            if !sources.contains_key(&SourceName::new(name))
                && !others.apps.contains_key(name)
                && !others.reports.contains_key(name)
                && !others.rules.contains_key(name)
            {
                let mut names: Vec<&str> = sources
                    .keys()
                    .map(SourceName::as_str)
                    .chain(others.apps.keys().map(String::as_str))
                    .chain(others.reports.keys().map(String::as_str))
                    .chain(others.rules.keys().map(String::as_str))
                    .collect();
                names.sort_unstable();
                let hint = if names.is_empty() {
                    format!(
                        "name it under [sources.{name}], [apps.{name}], [reports.{name}] or [rules.{name}]"
                    )
                } else {
                    format!("the sources, apps, reports and rules are {}", list(&names))
                };
                self.error(
                    &source.span(),
                    &format!("{path}.sources"),
                    format!("no source, app, report or rule is named \"{name}\"; {hint}"),
                );
                sound = false;
            }
        }
        sound
    }

    /// Whether `route` names only reports among its sources where it is
    /// living.
    fn lives(&mut self, route: &RouteFile, path: &str, others: Others<'_>) -> bool {
        if !route.living {
            return true;
        }
        let unreported: Vec<&str> = route
            .sources
            .iter()
            .map(|source| source.get_ref().as_str())
            .filter(|name| !others.reports.contains_key(*name))
            .collect();
        if unreported.is_empty() {
            return true;
        }
        self.error(
            &route.id.span(),
            &format!("{path}.living"),
            format!(
                "a living route keeps one post of each report it takes, so it names only reports among its sources; {} is not one",
                list(&unreported)
            ),
        );
        false
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
        (others, boards): (Others<'_>, Vec<BoardRoute<'_>>),
        templates: &BTreeMap<TemplateName, Written>,
    ) -> Vec<Read> {
        let mut routes = Vec::new();
        let mut ids: BTreeSet<RouteId> = BTreeSet::new();
        let written: Vec<BoardRoute<'_>> = self
            .routes(value)
            .into_iter()
            .enumerate()
            .map(|(index, value)| (format!("routes[{index}]"), value))
            .chain(boards)
            .collect();
        for (path, value) in written {
            let Some(read) = self.route(path, value, destinations, sources, others, templates)
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

    /// The template `route` renders with, where `templates` has the one it
    /// names: none where it is a route to a feed, which renders none.
    ///
    /// # Errors
    ///
    /// When a route to a feed names a template, another route names none,
    /// or `templates` has not the one it names.
    fn rendered(
        &mut self,
        route: &RouteFile,
        (path, span): (&str, &Range<usize>),
        feeds: bool,
        templates: &BTreeMap<TemplateName, Written>,
    ) -> Result<Option<Rendered>, ()> {
        let key = format!("{path}.template");
        let given = match (&route.template, feeds) {
            (None, true) => return Ok(None),
            (Some(given), false) => given,
            (Some(given), true) => {
                self.error(&given.span(), &key, "a herald feed or push carries each event as an envelope and renders no template; remove template");
                return Err(());
            }
            (None, false) => {
                self.error(
                    span,
                    &key,
                    "give the template the route renders with: template = \"<name>\"",
                );
                return Err(());
            }
        };
        let template = TemplateName::new(given.get_ref());
        let Some(written) = templates.get(&template) else {
            let names: Vec<&str> = templates.keys().map(TemplateName::as_str).collect();
            self.error(
                &given.span(),
                &key,
                format!(
                    "no template is named \"{}\"; the templates are {}",
                    given.get_ref(),
                    list(&names)
                ),
            );
            return Err(());
        };
        Ok(Some(Rendered {
            template,
            title_span: written.title.span(),
            body_span: written.body.span(),
            body_path: written.body_path.clone(),
        }))
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
        path: String,
        value: Value<'_>,
        destinations: &BTreeMap<DestinationName, (Destination, Range<usize>)>,
        sources: &BTreeMap<SourceName, Source>,
        others: Others<'_>,
        templates: &BTreeMap<TemplateName, Written>,
    ) -> Option<Read> {
        let (common, span, rest) = self.parted(value, &path, &ROUTE_KEYS)?;
        let rest = Spanned::new(span, DeValue::Table(rest));
        let route = self.typed::<RouteFile>(common, &path)?;
        let mut sound = self.sources_named(&route, &path, sources, others);
        let destination = DestinationName::new(route.destination.get_ref());
        let reached = self.reached(&destination, &route, &path, destinations)?;
        let span = route.id.span();
        let feeds = reached.feeding().is_some() || reached.pushing().is_some();
        let rendered = self.rendered(&route, (&path, &span), feeds, templates);
        let (options, name) =
            self.placed(reached, rest, &route, &path, &destination, &mut sound)?;
        let (profile, as_bot) = reached.profile(&options)?;
        let profile = Profile {
            symbols: reached.symbols(),
            ..profile
        };
        let thread_name = route.thread.as_ref().map(|thread| thread.name.clone());
        let (private, stated) = (route.private.unwrap_or_default(), route.private.is_some());
        sound &= self.lives(&route, &path, others);
        let (group, group_key) = self.grouping(route.group, (&path, &options, feeds)).unzip();
        let delivery = Delivery {
            crosspost: route.crosspost,
            thread: route.thread.map(opened),
            pin: pinned(route.pin, route.living, profile.features.pins),
            mentions: Mentions {
                roles: mentioned(route.mentions.roles, options.opt_in_role()),
                subscribers: route.mentions.subscribers,
            },
            late_release: route.late_release,
            edits: route.edits,
            withdraw: route.withdraw,
            group,
            living: route.living,
        };
        let reach = Reach {
            name,
            profile: &profile,
            as_bot: as_bot.as_ref(),
            threads_each_post: options.threads_each_post(),
        };
        sound &= self.deliverable(&path, &span, &destination, reach, &delivery);
        let (Ok(rendered), true) = (rendered, sound) else {
            return None;
        };
        let taken = Taken {
            sources: &route.sources,
            kinds: &route.kinds,
            versions: route.versions.as_ref(),
        };
        let sample_kinds = self.sample_kinds(taken, &path, sources, others)?;
        let matching = self.matching(route.matching, &path, &sample_kinds)?;
        let origins = route.sources.iter().all(|name| {
            sources
                .get(&SourceName::new(name.get_ref()))
                .is_some_and(|source| source.origins())
        });
        let filter_sources: Vec<SourceName> = route
            .sources
            .iter()
            .map(|source| SourceName::new(source.get_ref()))
            .collect();
        let filter_kinds: Vec<Kind> = route.kinds.into_iter().map(Spanned::into_inner).collect();
        let filter = Filter {
            sources: filter_sources,
            kinds: filter_kinds,
            prereleases: route.prereleases,
            versions: route
                .versions
                .map(|versions| Pattern::new(versions.into_inner())),
            matching,
        };
        let config = RouteConfig {
            route: Route {
                id: RouteId::new(route.id.get_ref()),
                filter,
                destination,
                template: rendered.as_ref().map(|rendered| rendered.template.clone()),
                delivery,
            },
            options,
            profile,
            private,
            stated,
            unstated: if stated {
                Vec::new()
            } else {
                self.private_kinds(&sample_kinds)
            },
        };
        Some(Read {
            config,
            rendered,
            sample_kinds,
            origins,
            thread: thread_name,
            group: group_key,
            id_span: span,
            path,
        })
    }
}

/// How a route pins as `pin` says, a living one where it says nothing
/// pinning by its marker where its destination pins as `pins` says.
fn pinned(pin: Option<PinFile>, living: bool, pins: Pins) -> Option<Pinning> {
    pin.map_or_else(
        || (living && pins != Pins::None).then(Pinning::default),
        PinFile::pinning,
    )
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
